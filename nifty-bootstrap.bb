#!/usr/bin/env bb

(require '[babashka.pods :as pods])
(pods/load-pod 'enigmacurry/script-wizard "0.3.0")

(require '[pod.enigmacurry.script-wizard :as wiz]
         '[babashka.process :as proc]
         '[babashka.http-client :as http]
         '[clojure.java.io :as io]
         '[clojure.string :as str]
         '[cheshire.core :as json])

;; ─── Constants ───────────────────────────────────────────────────────────────

(def nifty-manifest-url
  (or (System/getenv "NIFTY_MANIFEST_URL")
      "https://nifty-filter-pve.nyc3.digitaloceanspaces.com/manifest.json"))

(def nixos-manifest-url
  (or (System/getenv "NIXOS_MANIFEST_URL")
      "https://nixos-vm-template.nyc3.digitaloceanspaces.com/manifest.json"))

(def defaults
  {:infra-bridge  "vmbr2"
   :mgmt-subnet   "10.99.0.0/24"
   :step-ca-ip    "10.99.2.3"
   :services-ip   "10.99.2.2"
   :router-mgmt   "10.99.0.1"
   :step-ca-vmid  "100"
   :services-vmid "202"
   :router-vmid   "101"
   :storage       "local-lvm"
   :staging-dir   "/tmp/nifty-staging"})

;; ─── State ───────────────────────────────────────────────────────────────────

(def local-pve? (atom nil))
(def pve-host (atom nil))

;; ─── Utility functions ──────────────────────────────────────────────────────

(defn sh
  "Run a shell command, return {:out :err :exit}. Throws on non-zero exit."
  [& args]
  (let [cmd (str/join " " args)]
    (proc/shell {:out :string :err :string} "bash" "-c" cmd)))

(defn sh-ok
  "Run a shell command, return stdout trimmed. Throws on failure."
  [& args]
  (str/trim (:out (apply sh args))))

(defn sh-ok?
  "Run a shell command, return true if exit 0."
  [& args]
  (try
    (apply sh args)
    true
    (catch Exception _ false)))

(defn pve-cmd!
  "Run a command on the PVE host (locally or via SSH)."
  [& args]
  (let [cmd (str/join " " args)]
    (if @local-pve?
      (sh-ok cmd)
      (sh-ok (format "ssh root@%s '%s'" @pve-host (str/replace cmd "'" "'\\''"))))))

(defn pve-cmd-ok?
  "Run a command on PVE, return true if success."
  [& args]
  (try (apply pve-cmd! args) true
       (catch Exception _ false)))

(defn qm!
  "Run a qm command on PVE."
  [& args]
  (apply pve-cmd! "qm" args))

(defn rsync-to-pve!
  "rsync a local file to the PVE host."
  [local-path remote-path]
  (if @local-pve?
    (sh-ok (format "cp '%s' '%s'" local-path remote-path))
    (sh-ok (format "rsync -ah --progress '%s' 'root@%s:%s'"
                   local-path @pve-host remote-path))))

(defn fetch-json
  "Fetch and parse JSON from a URL."
  [url]
  (let [resp (http/get url {:headers {"Accept" "application/json"}})]
    (json/parse-string (:body resp) true)))

(defn download-image!
  "Download a file from URL to local path with progress."
  [url dest]
  (println (format "  Downloading: %s" url))
  (sh (format "curl -fL --progress-bar -o '%s' '%s'" dest url))
  dest)

(defn verify-sha256!
  "Verify sha256 checksum of a file. Throws on mismatch."
  [path expected]
  (let [actual (first (str/split (sh-ok (format "sha256sum '%s'" path)) #"\s+"))]
    (when (not= actual expected)
      (throw (ex-info (format "SHA256 mismatch for %s\n  expected: %s\n  actual:   %s"
                              path expected actual)
                      {:expected expected :actual actual})))
    (println "  Checksum verified.")))

(defn generate-machine-id
  "Generate a 32-hex-char machine-id."
  []
  (str/replace (str (java.util.UUID/randomUUID)) "-" ""))

(defn subnet-gateway
  "Derive .1 gateway from an IP address string."
  [ip]
  (str (str/join "." (concat (butlast (str/split ip #"\.")) ["1"]))))

(defn subnet-pve-ip
  "Derive .2 PVE address from a subnet like 10.99.0.0/24."
  [subnet]
  (let [[net prefix] (str/split subnet #"/")
        base (str/join "." (butlast (str/split net #"\.")))]
    (str base ".2/" prefix)))

;; ─── Bridge management ──────────────────────────────────────────────────────

(defn ensure-bridge!
  "Create a Linux bridge on PVE if it doesn't exist. Optionally assign an IP."
  [name & {:keys [ip]}]
  (if (pve-cmd-ok? (format "ip link show %s" name))
    (println (format "  Bridge %s already exists." name))
    (do
      (println (format "  Creating bridge %s on PVE..." name))
      (let [iface-block (if ip
                          (format "auto %s\niface %s inet static\n    address %s\n    bridge-ports none\n    bridge-stp off\n    bridge-fd 0"
                                  name name ip)
                          (format "auto %s\niface %s inet manual\n    bridge-ports none\n    bridge-stp off\n    bridge-fd 0"
                                  name name))]
        (pve-cmd! (format "printf '\\n%s\\n' >> /etc/network/interfaces && ifup %s"
                          iface-block name)))
      (println (format "  Bridge %s created." name)))))

;; ─── Identity population (guestfish) ────────────────────────────────────────

(defn write-identity-files!
  "Write identity files to a temp dir for guestfish copy-in."
  [tmp-dir {:keys [hostname machine-id ssh-keys tcp-ports udp-ports
                   resolv-conf hosts static-ip]}]
  (.mkdirs (io/file tmp-dir))
  (spit (str tmp-dir "/hostname") hostname)
  (spit (str tmp-dir "/machine-id") machine-id)
  (spit (str tmp-dir "/admin_authorized_keys") (str ssh-keys "\n"))
  (spit (str tmp-dir "/user_authorized_keys") (str ssh-keys "\n"))
  (when tcp-ports
    (spit (str tmp-dir "/tcp_ports") (str/join "\n" tcp-ports)))
  (when udp-ports
    (spit (str tmp-dir "/udp_ports") (str/join "\n" udp-ports)))
  (when resolv-conf
    (spit (str tmp-dir "/resolv.conf") resolv-conf))
  (when hosts
    (spit (str tmp-dir "/hosts") hosts))
  (when static-ip
    (spit (str tmp-dir "/static_ip") static-ip))
  ;; Empty root password hash (no root password)
  (spit (str tmp-dir "/root_password_hash") ""))

(defn guestfish-populate-var!
  "Populate a var disk image with identity files using guestfish on PVE."
  [var-disk-path tmp-dir identity]
  ;; Write identity files locally
  (write-identity-files! tmp-dir identity)
  ;; Transfer tmp-dir to PVE if remote
  (let [remote-tmp (str (:staging-dir defaults) "/identity-" (:hostname identity))]
    (if @local-pve?
      (do
        (sh-ok (format "mkdir -p '%s'" remote-tmp))
        (sh-ok (format "cp -r '%s/'* '%s/'" tmp-dir remote-tmp)))
      (do
        (pve-cmd! (format "mkdir -p '%s'" remote-tmp))
        (sh-ok (format "rsync -a '%s/' 'root@%s:%s/'" tmp-dir @pve-host remote-tmp))))
    ;; Build guestfish command
    (let [gf-cmds (str "run"
                       " : part-disk /dev/sda gpt"
                       " : mkfs ext4 /dev/sda1"
                       " : mount /dev/sda1 /"
                       " : mkdir-p /identity"
                       (format " : copy-in %s/hostname /identity/" remote-tmp)
                       (format " : copy-in %s/machine-id /identity/" remote-tmp)
                       (format " : copy-in %s/admin_authorized_keys /identity/" remote-tmp)
                       " : chmod 0644 /identity/admin_authorized_keys"
                       " : chown 0 0 /identity/admin_authorized_keys"
                       (format " : copy-in %s/user_authorized_keys /identity/" remote-tmp)
                       " : chmod 0644 /identity/user_authorized_keys"
                       " : chown 0 0 /identity/user_authorized_keys"
                       (when (:tcp-ports identity)
                         (str (format " : copy-in %s/tcp_ports /identity/" remote-tmp)
                              " : chmod 0644 /identity/tcp_ports"
                              " : chown 0 0 /identity/tcp_ports"))
                       (when (:udp-ports identity)
                         (str (format " : copy-in %s/udp_ports /identity/" remote-tmp)
                              " : chmod 0644 /identity/udp_ports"
                              " : chown 0 0 /identity/udp_ports"))
                       (when (:resolv-conf identity)
                         (str (format " : copy-in %s/resolv.conf /identity/" remote-tmp)
                              " : chmod 0644 /identity/resolv.conf"
                              " : chown 0 0 /identity/resolv.conf"))
                       (when (:hosts identity)
                         (str (format " : copy-in %s/hosts /identity/" remote-tmp)
                              " : chmod 0644 /identity/hosts"
                              " : chown 0 0 /identity/hosts"))
                       (when (:static-ip identity)
                         (str (format " : copy-in %s/static_ip /identity/" remote-tmp)
                              " : chmod 0644 /identity/static_ip"
                              " : chown 0 0 /identity/static_ip"))
                       (format " : copy-in %s/root_password_hash /identity/" remote-tmp)
                       " : chmod 0600 /identity/root_password_hash"
                       " : chown 0 0 /identity/root_password_hash")]
      (println "  Populating /var disk with identity...")
      (pve-cmd! (format "LIBGUESTFS_BACKEND=direct guestfish -a '%s' %s" var-disk-path gf-cmds))
      ;; Cleanup
      (pve-cmd! (format "rm -rf '%s'" remote-tmp)))))

;; ─── Deploy: nixos-vm-template VM (step-ca or infra-services) ───────────────

(defn deploy-nixos-vm!
  "Deploy a nixos-vm-template VM (step-ca or infra-services)."
  [{:keys [vm-name vmid image-url image-sha256 bridge ip
           memory cores var-size ssh-keys identity storage]}]
  (let [staging (:staging-dir defaults)
        image-file (format "%s/%s.qcow2" staging vm-name)
        var-file (format "%s/%s-var.qcow2" staging vm-name)
        local-tmp (format "/tmp/nifty-bootstrap-%s" vm-name)]
    (println)
    (println (format "=== Deploying %s (VMID %s) ===" vm-name vmid))

    ;; Download image
    (println "Downloading image...")
    (.mkdirs (io/file "/tmp"))
    (let [local-image (format "/tmp/nifty-bootstrap-%s.qcow2" vm-name)]
      (download-image! image-url local-image)
      (verify-sha256! local-image image-sha256)

      ;; Ensure staging dir on PVE
      (pve-cmd! (format "mkdir -p '%s'" staging))

      ;; Transfer image to PVE
      (println "  Uploading image to PVE...")
      (rsync-to-pve! local-image image-file)
      (io/delete-file local-image true))

    ;; Create var disk on PVE
    (println (format "  Creating %s var disk..." var-size))
    (pve-cmd! (format "qemu-img create -f qcow2 '%s' %s" var-file var-size))

    ;; Populate var disk with identity
    (guestfish-populate-var! var-file local-tmp identity)

    ;; Create VM
    (println (format "  Creating VM %s..." vmid))
    (qm! (format "create %s --name %s --bios ovmf --machine q35 --cpu host --agent 1 --cores %s --memory %s --efidisk0 %s:1,efitype=4m,pre-enrolled-keys=0,format=raw --serial0 socket --vga serial0 --net0 virtio,bridge=%s"
                  vmid vm-name cores memory storage bridge))

    ;; Import boot disk
    (println "  Importing boot disk...")
    (qm! (format "importdisk %s '%s' %s --format raw" vmid image-file storage))

    ;; Import var disk
    (println "  Importing var disk...")
    (qm! (format "importdisk %s '%s' %s --format raw" vmid var-file storage))

    ;; Attach disks
    (println "  Attaching disks and setting boot order...")
    (let [config (qm! (format "config %s" vmid))
          boot-vol (some->> (str/split-lines config)
                            (filter #(str/starts-with? % "unused0:"))
                            first
                            (re-find #"unused0: (.+)")
                            second
                            str/trim)
          var-vol (some->> (str/split-lines config)
                           (filter #(str/starts-with? % "unused1:"))
                           first
                           (re-find #"unused1: (.+)")
                           second
                           str/trim)]
      (qm! (format "set %s --virtio0 %s --virtio1 %s --boot order=virtio0"
                    vmid boot-vol var-vol)))

    ;; Cleanup staging
    (pve-cmd! (format "rm -f '%s' '%s'" image-file var-file))

    ;; Start VM
    (println (format "  Starting VM %s..." vmid))
    (qm! (format "start %s" vmid))
    (println (format "  %s (VMID %s) is running." vm-name vmid))))

;; ─── Deploy: nifty-filter router VM ─────────────────────────────────────────

(defn pci-device?
  "Check if a NIC string is a PCI address (e.g. 01:00.0 or 0000:01:00.0)."
  [nic]
  (boolean (re-matches #"(?:0000:)?[0-9a-fA-F]{2}:[0-9a-fA-F]{2}\.[0-9a-fA-F]" nic)))

(defn normalize-pci
  "Strip leading 0000: prefix if present, for hostpci flags."
  [addr]
  (str/replace addr #"^0000:" ""))

(defn deploy-nifty-filter!
  "Deploy the nifty-filter router VM.
   nics is a seq of NIC identifiers: PCI addresses (e.g. '01:00.0') or bridge names (e.g. 'vmbr0').
   The first NIC is the WAN, remaining are extras. mgmt and infra are added automatically."
  [{:keys [vmid image-url image-sha256 nics infra-bridge
           mgmt-subnet ssh-keys storage]}]
  (let [staging (:staging-dir defaults)
        image-file (format "%s/nifty-filter.qcow2" staging)
        mgmt-prefix (second (str/split mgmt-subnet #"/"))
        mgmt-base (str/join "." (butlast (str/split (first (str/split mgmt-subnet #"/")) #"\.")))
        pve-mgmt-ip (str mgmt-base ".2/" mgmt-prefix)
        ;; Classify NICs as PCI passthrough or virtual bridges
        pci-devices (filterv pci-device? nics)
        bridges (filterv (complement pci-device?) nics)]
    (println)
    (println (format "=== Deploying nifty-filter (VMID %s) ===" vmid))

    ;; Download image
    (println "Downloading image...")
    (let [local-image "/tmp/nifty-bootstrap-nifty-filter.qcow2"]
      (download-image! image-url local-image)
      (verify-sha256! local-image image-sha256)

      ;; Transfer to PVE
      (pve-cmd! (format "mkdir -p '%s'" staging))
      (println "  Uploading image to PVE...")
      (rsync-to-pve! local-image image-file)
      (io/delete-file local-image true))

    ;; Ensure mgmt bridge
    (println "Setting up mgmt bridge...")
    (ensure-bridge! "mgmt" :ip pve-mgmt-ip)

    ;; Ensure user-specified bridges exist
    (doseq [bridge bridges]
      (ensure-bridge! bridge))

    ;; Build NIC args: net0=mgmt, then virtual bridges, then infra (last virtual NIC)
    ;; PCI devices get --hostpci flags instead of --net
    (let [net-args (str "--net0 virtio,bridge=mgmt"
                        (str/join ""
                          (map-indexed (fn [i bridge]
                                        (format " --net%d virtio,bridge=%s" (inc i) bridge))
                                      bridges))
                        ;; Infra NIC is always the last virtual NIC
                        (format " --net%d virtio,bridge=%s"
                                (+ 1 (count bridges)) infra-bridge))
          hostpci-args (str/join ""
                         (map-indexed (fn [i dev]
                                       (format " --hostpci%d 0000:%s,pcie=1" i (normalize-pci dev)))
                                     pci-devices))
          infra-net-index (+ 1 (count bridges))]

      ;; Create VM
      (println (format "  Creating VM %s..." vmid))
      (qm! (format "create %s --name nifty-filter --machine q35 --bios ovmf --cpu host --cores 2 --memory 2048 --efidisk0 %s:1,efitype=4m,pre-enrolled-keys=0 --scsihw virtio-scsi-single --ostype l26 --onboot 1 --serial0 socket --vga serial0 %s%s"
                    vmid storage net-args hostpci-args))

      ;; Import boot disk as scsi0
      (println "  Importing boot disk...")
      (qm! (format "importdisk %s '%s' %s" vmid image-file storage))
      (qm! (format "set %s --scsi0 %s:vm-%s-disk-1 --boot order=scsi0" vmid storage vmid))

      ;; Create and format /var disk as scsi1
      (println "  Creating 8G /var disk...")
      (qm! (format "set %s --scsi1 %s:8" vmid storage))
      (let [config (qm! (format "config %s" vmid))
            var-volid (some->> (str/split-lines config)
                               (filter #(str/starts-with? % "scsi1:"))
                               first
                               (re-find #"scsi1: ([^,]+)")
                               second
                               str/trim)
            var-path (pve-cmd! (format "pvesm path %s" var-volid))]
        (println "  Formatting /var disk (NIFTY_VAR)...")
        (pve-cmd! (format "mkfs.ext4 -F -L NIFTY_VAR -q '%s'" var-path))

        ;; Inject SSH keys into var disk via guestfish
        (when (not (str/blank? ssh-keys))
          (println "  Injecting SSH keys into /var disk...")
          (let [key-dir (format "%s/nifty-keys" staging)]
            (pve-cmd! (format "mkdir -p '%s'" key-dir))
            ;; Write keys to PVE
            (if @local-pve?
              (spit (str key-dir "/authorized_keys") (str ssh-keys "\n"))
              (sh-ok (format "echo '%s' | ssh root@%s 'cat > %s/authorized_keys'"
                             (str/replace ssh-keys "'" "'\\''")
                             @pve-host key-dir)))
            ;; Use guestfish to inject into var disk
            (pve-cmd! (format "LIBGUESTFS_BACKEND=direct guestfish -a '%s' run : mount /dev/sda / : mkdir-p /home/admin/.ssh : copy-in %s/authorized_keys /home/admin/.ssh/ : chmod 0700 /home/admin/.ssh : chmod 0600 /home/admin/.ssh/authorized_keys : chown 1000 100 /home/admin/.ssh : chown 1000 100 /home/admin/.ssh/authorized_keys"
                              var-path key-dir))
            (pve-cmd! (format "rm -rf '%s'" key-dir)))))

      ;; Build NIC role order and fw_cfg args
      ;; Roles: first NIC is wan, then trunk, then extra1, extra2, ...
      ;; PCI NICs are discovered by bus address order at boot (no MAC needed here)
      ;; Virtual bridge NICs get their MACs passed via fw_cfg
      (println "  Configuring fw_cfg parameters...")
      (let [config (qm! (format "config %s" vmid))
            get-mac (fn [net-key]
                      (some->> (str/split-lines config)
                               (filter #(str/starts-with? % (str net-key ":")))
                               first
                               (re-find #"virtio=([^,]+)")
                               second))
            mgmt-mac (get-mac "net0")
            ;; Build role list matching NIC order (wan first, then extras)
            nic-roles (str/join ":"
                        (concat ["wan" "trunk"]
                                (map #(str "extra" (inc %))
                                     (range (max 0 (- (count nics) 2))))))
            infra-mac (get-mac (format "net%d" infra-net-index))
            ;; Start building fw_cfg
            fw-cfg (atom (str (format "-fw_cfg name=opt/nifty/mgmt_mac,string=%s" mgmt-mac)
                              (format " -fw_cfg name=opt/nifty/nic_roles,string=%s" nic-roles)))]
        (println (format "    mgmt MAC:   %s" mgmt-mac))
        (println (format "    NIC roles:  %s" nic-roles))

        ;; Pass MACs for virtual bridge NICs (PCI NICs are discovered by bus address)
        (loop [bridge-idx 0
               nic-idx 0
               role-names (concat ["wan" "trunk"]
                                  (map #(str "extra" (inc %))
                                       (range (max 0 (- (count nics) 2)))))]
          (when (and (< nic-idx (count nics)) (seq role-names))
            (let [nic (nth nics nic-idx)]
              (if (pci-device? nic)
                ;; PCI device — no MAC to pass, skip to next role
                (recur bridge-idx (inc nic-idx) (rest role-names))
                ;; Virtual bridge — read MAC and pass via fw_cfg
                (let [net-key (format "net%d" (inc bridge-idx))
                      mac (get-mac net-key)
                      role (first role-names)]
                  (when mac
                    (swap! fw-cfg str (format " -fw_cfg name=opt/nifty/%s_mac,string=%s" role mac))
                    (println (format "    %s MAC:  %s (bridge)" role mac)))
                  (recur (inc bridge-idx) (inc nic-idx) (rest role-names)))))))

        ;; Infra NIC MAC
        (when infra-mac
          (swap! fw-cfg str (format " -fw_cfg name=opt/nifty/infra_mac,string=%s" infra-mac))
          (println (format "    infra MAC:  %s (bridge: %s)" infra-mac infra-bridge)))

        (qm! (format "set %s --args '%s'" vmid @fw-cfg))))

    ;; Cleanup staging
    (pve-cmd! (format "rm -f '%s'" image-file))

    ;; Start VM
    (println (format "  Starting VM %s..." vmid))
    (qm! (format "start %s" vmid))
    (println (format "  nifty-filter (VMID %s) is running." vmid))))

;; ─── Main ───────────────────────────────────────────────────────────────────

(defn -main []
  (println)
  (println "  nifty-filter bootstrap")
  (println "  ~~~~~~~~~~~~~~~~~~~~~~")
  (println "  Deploy nifty-filter infrastructure VMs to Proxmox VE")
  (println "  from pre-built images (no Nix required).")
  (println)

  ;; Step 1: PVE connection
  (let [on-pve? (.exists (io/file "/usr/sbin/qm"))]
    (reset! local-pve? on-pve?)
    (if on-pve?
      (do
        (println "  Detected: running on Proxmox VE host.")
        (reset! pve-host "localhost"))
      (do
        (let [ssh-hosts (try
                          (->> (slurp (str (System/getenv "HOME") "/.ssh/config"))
                               str/split-lines
                               (keep #(second (re-find #"(?i)^\s*Host\s+(.+)" %)))
                               (mapcat #(str/split % #"\s+"))
                               (remove #(str/includes? % "*"))
                               vec)
                          (catch Exception _ []))]
          (reset! pve-host (wiz/ask "Proxmox VE host (hostname or IP):"
                                    :suggestions ssh-hosts)))
        (println (format "  Testing SSH connection to root@%s..." @pve-host))
        (try
          (sh-ok (format "ssh -o ConnectTimeout=10 root@%s 'pveversion'" @pve-host))
          (println "  SSH connection OK.")
          (catch Exception e
            (println (format "  ERROR: Cannot SSH to root@%s" @pve-host))
            (println "  Ensure you can: ssh root@<pve-host>")
            (System/exit 1))))))

  ;; Step 2: Fetch manifests
  (println)
  (println "Fetching image manifests...")
  (let [nifty-manifest (try (fetch-json nifty-manifest-url)
                            (catch Exception e
                              (println (format "  WARNING: Could not fetch nifty-filter manifest: %s"
                                               (.getMessage e)))
                              nil))
        nixos-manifest (try (fetch-json nixos-manifest-url)
                            (catch Exception e
                              (println (format "  WARNING: Could not fetch nixos-vm-template manifest: %s"
                                               (.getMessage e)))
                              nil))
        ;; Extract available images
        nf-image (get-in nifty-manifest [:images :nifty-filter])
        step-ca-image (get-in nixos-manifest [:profiles :step-ca])
        services-image (or (get-in nixos-manifest [:profiles (keyword "podman,nifty-services")])
                           ;; Try string key lookup
                           (get (:profiles nixos-manifest) (keyword "podman,nifty-services")))]

    ;; Display available images
    (println)
    (println "Available images:")
    (when nf-image
      (println (format "  nifty-filter:    %s (commit %s)" (:date nf-image) (:commit nf-image))))
    (when step-ca-image
      (println (format "  step-ca:         %s (commit %s)" (:date step-ca-image) (:commit step-ca-image))))
    (when services-image
      (println (format "  infra-services:  %s (commit %s)" (:date services-image) (:commit services-image))))
    (println)

    ;; Build selection options based on available images
    (let [available (cond-> []
                      step-ca-image  (conj "infra-CA (step-ca)")
                      services-image (conj "infra-services")
                      nf-image       (conj "nifty-filter"))]
      (when (empty? available)
        (println "ERROR: No images available in either manifest.")
        (System/exit 1))

      ;; Step 3: Select VMs
      (let [selected (wiz/select "Which VMs to deploy?" available :default available)
            deploy-step-ca?  (some #(str/starts-with? % "infra-CA") selected)
            deploy-services? (some #(= % "infra-services") selected)
            deploy-router?   (some #(= % "nifty-filter") selected)]

        ;; Step 4: Network configuration
        (println)
        (let [infra-bridge (wiz/ask "Infrastructure bridge name:" :default (:infra-bridge defaults))
              mgmt-subnet  (if deploy-router?
                             (wiz/ask "Management subnet (CIDR):" :default (:mgmt-subnet defaults))
                             (:mgmt-subnet defaults))
              step-ca-ip   (if deploy-step-ca?
                             (wiz/ask "Step-CA IP address:" :default (:step-ca-ip defaults))
                             (:step-ca-ip defaults))
              services-ip  (if deploy-services?
                             (wiz/ask "Infra-services IP address:" :default (:services-ip defaults))
                             (:services-ip defaults))
              ;; Router NICs: PCI passthrough addresses or bridge names
              ;; First NIC = WAN, second = trunk, rest = extras
              ;; mgmt and infra NICs are added automatically
              router-nics  (if deploy-router?
                             (let [;; Query PCI network devices from PVE
                                   pci-output (try (pve-cmd! "lspci -Dnn | grep -i 'ethernet\\|network' | while IFS= read -r line; do pci=$(echo \"$line\" | awk '{print $1}'); desc=$(echo \"$line\" | cut -d' ' -f2-); mac=$(cat /sys/bus/pci/devices/$pci/net/*/address 2>/dev/null | head -1); printf '%s\\t%s\\t%s\\n' \"$pci\" \"$mac\" \"$desc\"; done")
                                                   (catch Exception _ ""))
                                   pci-devices (vec (keep (fn [line]
                                                            (let [parts (str/split line #"\t" 3)]
                                                              (when (and (seq parts) (not (str/blank? (first parts))))
                                                                {:id (first parts)
                                                                 :mac (second parts)
                                                                 :desc (get parts 2 "")})))
                                                          (str/split-lines pci-output)))
                                   ;; Query bridges from PVE
                                   bridge-output (try (pve-cmd! "ip -br link show type bridge | awk '{print $1, $3}'")
                                                      (catch Exception _ ""))
                                   bridges (vec (keep (fn [line]
                                                        (let [parts (str/split (str/trim line) #"\s+")]
                                                          (when (and (seq parts) (not (str/blank? (first parts))))
                                                            {:name (first parts)
                                                             :mac (get parts 1 "")})))
                                                      (str/split-lines bridge-output)))]
                               (println)
                               (println "Router NIC configuration:")
                               (println "  First NIC = WAN, second = trunk, rest = extras.")
                               (println "  mgmt and infra NICs are added automatically.")
                               (when (seq pci-devices)
                                 (println)
                                 (println "  PCI network devices on PVE:")
                                 (doseq [d pci-devices]
                                   (println (format "    %s %s %s"
                                                    (:id d)
                                                    (if (str/blank? (:mac d)) "" (str "[" (:mac d) "]"))
                                                    (:desc d)))))
                               (when (seq bridges)
                                 (println)
                                 (println "  Bridges on PVE:")
                                 (doseq [b bridges]
                                   (println (format "    %s %s" (:name b) (if (str/blank? (:mac b)) "" (str "[" (:mac b) "]"))))))
                               (println)
                               (let [;; Build choosable options
                                     pci-options (mapv (fn [d]
                                                         (format "PCI: %s %s %s"
                                                                 (:id d)
                                                                 (if (str/blank? (:mac d)) "" (str "[" (:mac d) "]"))
                                                                 (:desc d)))
                                                       pci-devices)
                                     bridge-options (mapv (fn [b]
                                                            (format "Bridge: %s %s"
                                                                    (:name b)
                                                                    (if (str/blank? (:mac b)) "" (str "[" (:mac b) "]"))))
                                                          bridges)
                                     all-options (into pci-options bridge-options)
                                     parse-choice (fn [choice]
                                                    (cond
                                                      (str/starts-with? choice "PCI: ")
                                                      (let [id (second (re-find #"PCI: (\S+)" choice))]
                                                        ;; Strip 0000: domain prefix
                                                        (str/replace id #"^0000:" ""))
                                                      (str/starts-with? choice "Bridge: ")
                                                      (second (re-find #"Bridge: (\S+)" choice))
                                                      :else choice))]
                                 (loop [nics []
                                        labels ["WAN" "Trunk"]]
                                   (let [label (or (first labels)
                                                   (format "Extra NIC #%d" (- (count nics) 1)))
                                         ;; After first 2 NICs, ask if they want more
                                         continue? (or (< (count nics) 2)
                                                       (wiz/confirm (format "Add another NIC? (%s)" label) :default :no))]
                                     (if (not continue?)
                                       nics
                                       (let [choice (wiz/choose (format "%s NIC:" label) all-options)
                                             nic-id (parse-choice choice)]
                                         (recur (conj nics nic-id) (rest labels))))))))
                             [])

              ;; Step 5: VMIDs
              step-ca-vmid  (if deploy-step-ca?
                              (wiz/ask "Step-CA VMID:" :default (:step-ca-vmid defaults))
                              (:step-ca-vmid defaults))
              services-vmid (if deploy-services?
                              (wiz/ask "Infra-services VMID:" :default (:services-vmid defaults))
                              (:services-vmid defaults))
              router-vmid   (if deploy-router?
                              (wiz/ask "Nifty-filter VMID:" :default (:router-vmid defaults))
                              (:router-vmid defaults))

              ;; Step 6: SSH keys
              agent-keys (try (sh-ok "ssh-add -L") (catch Exception _ ""))
              ssh-keys (if (str/blank? agent-keys)
                         (let [key-path (wiz/ask "Path to SSH public key:"
                                                 :default (str (System/getenv "HOME") "/.ssh/id_ed25519.pub"))]
                           (str/trim (slurp key-path)))
                         (do
                           (println)
                           (println "SSH keys from agent:")
                           (doseq [line (str/split-lines agent-keys)]
                             (let [parts (str/split line #"\s+" 3)
                                   comment (get parts 2 "")]
                               (println (format "  %s ...%s %s"
                                                (first parts)
                                                (subs (second parts) (max 0 (- (count (second parts)) 12)))
                                                comment))))
                           (if (wiz/confirm "Use these SSH keys?" :default :yes)
                             agent-keys
                             (let [key-path (wiz/ask "Path to SSH public key:"
                                                     :default (str (System/getenv "HOME") "/.ssh/id_ed25519.pub"))]
                               (str/trim (slurp key-path))))))

              ;; Step 7: Storage
              storage (wiz/ask "PVE storage backend:" :default (:storage defaults)
                               :suggestions ["local-lvm" "local-zfs" "local"])

              ;; Derived values
              infra-gateway (subnet-gateway step-ca-ip)
              mgmt-base (str/join "." (butlast (str/split (first (str/split mgmt-subnet #"/")) #"\.")))]

          ;; Step 8: Confirmation
          (println)
          (println "=== Deployment Summary ===")
          (println)
          (println (format "  PVE host:    %s%s" @pve-host (if @local-pve? " (local)" "")))
          (println (format "  Storage:     %s" storage))
          (println (format "  SSH keys:    %d key(s)" (count (str/split-lines ssh-keys))))
          (println)
          (when deploy-step-ca?
            (println (format "  infra-CA:       VMID %s, %s/%s on %s"
                             step-ca-vmid step-ca-ip "24" infra-bridge)))
          (when deploy-services?
            (println (format "  infra-services: VMID %s, %s/%s on %s"
                             services-vmid services-ip "24" infra-bridge)))
          (when deploy-router?
            (println (format "  nifty-filter:   VMID %s, %s on mgmt, infra on %s"
                             router-vmid (str mgmt-base ".1") infra-bridge))
            (doseq [[i nic] (map-indexed vector router-nics)]
              (println (format "                  NIC %d: %s%s"
                               i nic (if (pci-device? nic) " (PCI passthrough)" " (bridge)")))))
          (println)

          (when-not (wiz/confirm "Proceed with deployment?" :default :yes)
            (println "Aborted.")
            (System/exit 0))

          ;; ─── Deploy ───────────────────────────────────────────────────

          ;; Ensure infra bridge exists
          (when (or deploy-step-ca? deploy-services?)
            (println)
            (println "Setting up infrastructure bridge...")
            (ensure-bridge! infra-bridge))

          ;; Deploy infra-CA
          (when deploy-step-ca?
            (deploy-nixos-vm!
             {:vm-name   "infra-CA"
              :vmid      step-ca-vmid
              :image-url (:url step-ca-image)
              :image-sha256 (:sha256 step-ca-image)
              :bridge    infra-bridge
              :ip        step-ca-ip
              :memory    "512"
              :cores     "1"
              :var-size  "4G"
              :ssh-keys  ssh-keys
              :storage   storage
              :identity  {:hostname    "infra-CA"
                          :machine-id  (generate-machine-id)
                          :ssh-keys    ssh-keys
                          :tcp-ports   ["22" "9443"]
                          :resolv-conf (format "nameserver %s\nnameserver 1.1.1.1" services-ip)
                          :static-ip   (format "address=%s/24\ngateway=%s" step-ca-ip infra-gateway)}}))

          ;; Deploy infra-services
          (when deploy-services?
            (deploy-nixos-vm!
             {:vm-name   "infra-services"
              :vmid      services-vmid
              :image-url (:url services-image)
              :image-sha256 (:sha256 services-image)
              :bridge    infra-bridge
              :ip        services-ip
              :memory    "2048"
              :cores     "2"
              :var-size  "8G"
              :ssh-keys  ssh-keys
              :storage   storage
              :identity  {:hostname    "infra-services"
                          :machine-id  (generate-machine-id)
                          :ssh-keys    ssh-keys
                          :tcp-ports   ["22" "53" "80" "443"]
                          :udp-ports   ["53" "123"]
                          :resolv-conf (format "nameserver %s" infra-gateway)
                          :hosts       (format "%s router.nifty.internal" infra-gateway)
                          :static-ip   (format "address=%s/24\ngateway=%s" services-ip infra-gateway)}}))

          ;; Deploy nifty-filter
          (when deploy-router?
            ;; Ensure infra bridge for router's infra NIC
            (ensure-bridge! infra-bridge)
            (deploy-nifty-filter!
             {:vmid       router-vmid
              :image-url  (:url nf-image)
              :image-sha256 (:sha256 nf-image)
              :nics       router-nics
              :infra-bridge infra-bridge
              :mgmt-subnet mgmt-subnet
              :ssh-keys   ssh-keys
              :storage    storage}))

          ;; ─── Post-deploy summary ──────────────────────────────────────
          (println)
          (println "=== Deployment Complete ===")
          (println)
          (when deploy-step-ca?
            (println (format "  infra-CA:       %s (VMID %s) on %s" step-ca-ip step-ca-vmid infra-bridge)))
          (when deploy-services?
            (println (format "  infra-services: %s (VMID %s) on %s" services-ip services-vmid infra-bridge)))
          (when deploy-router?
            (println (format "  nifty-filter:   %s (VMID %s) on mgmt" (str mgmt-base ".1") router-vmid)))
          (println)
          (println "Next steps:")
          (when deploy-router?
            (if @local-pve?
              (println (format "  1. SSH to router: ssh admin@%s.1" mgmt-base))
              (println (format "  1. SSH to router: ssh -J root@%s admin@%s.1" @pve-host mgmt-base)))
            (println "  2. Configure router: nifty-config"))
          (when (or deploy-step-ca? deploy-services? deploy-router?)
            (println "  3. Distribute certs (from workstation with nifty-filter repo):")
            (println (format "     just pve-distribute-certs %s" @pve-host)))
          (println))))))

(-main)
