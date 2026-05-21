# Boot-time initialization services:
#   - nifty-filter-init: seeds default HCL config on first boot
#   - nifty-config-sha: snapshots config hash for drift detection
#   - nifty-hostname: sets hostname from HCL config
#   - nifty-link: generates .link files for interface renaming
#
# All run as oneshot services early in boot before networking comes up.

{ pkgs, nifty-filter, configDir, hclFile, ... }:

let
  acl = pkgs.acl;
in

{
  # Seed the default config on first boot if it doesn't exist
  systemd.services.nifty-filter-init = {
    description = "Initialize nifty-filter default configuration";
    wantedBy = [ "multi-user.target" ];
    before = [ "nifty-filter.service" ];
    unitConfig.ConditionPathExists = "!${hclFile}";
    serviceConfig = {
      Type = "oneshot";
      RemainAfterExit = true;
    };
    script = ''
      mkdir -p ${configDir}
      chown root:wheel ${configDir}
      chmod 0770 ${configDir}
      ${acl}/bin/setfacl -m g:nifty-config:rx ${configDir}
      cp ${../../examples/vlan_router.hcl} ${hclFile}
      chmod 0660 ${hclFile}
      chown root:wheel ${hclFile}
      ${acl}/bin/setfacl -m g:nifty-config:r ${hclFile}
      mkdir -p ${configDir}/ssh
      chmod 0700 ${configDir}/ssh
    '';
  };

  # Snapshot the config file SHA at boot so the dashboard can detect drift
  systemd.services.nifty-config-sha = {
    description = "Record config SHA256 at boot";
    wantedBy = [ "multi-user.target" ];
    after = [ "nifty-filter-init.service" ];
    before = [ "nifty-filter.service" ];
    serviceConfig = {
      Type = "oneshot";
      RemainAfterExit = true;
      RuntimeDirectory = "nifty-filter";
      RuntimeDirectoryPreserve = "yes";
    };
    script = ''
      if [ -f ${hclFile} ]; then
        ${pkgs.coreutils}/bin/sha256sum ${hclFile} \
          | ${pkgs.coreutils}/bin/cut -d' ' -f1 \
          > /run/nifty-filter/config-boot-sha
        ${pkgs.coreutils}/bin/cp ${hclFile} /run/nifty-filter/config-boot-snapshot
      else
        echo "" > /run/nifty-filter/config-boot-sha
        echo "" > /run/nifty-filter/config-boot-snapshot
      fi
      ${pkgs.coreutils}/bin/chmod 0444 /run/nifty-filter/config-boot-sha /run/nifty-filter/config-boot-snapshot
    '';
  };

  # Set hostname from HCL config at boot
  systemd.services.nifty-hostname = {
    description = "Set hostname from nifty-filter HCL config";
    wantedBy = [ "multi-user.target" ];
    after = [ "nifty-filter-init.service" "local-fs.target" ];
    serviceConfig = {
      Type = "oneshot";
      RemainAfterExit = true;
    };
    path = [ pkgs.hostname ];
    script = ''
      if [ -f ${hclFile} ]; then
        NAME=$(${nifty-filter}/bin/nifty-filter hostname --config ${hclFile} 2>/dev/null)
        if [ -n "$NAME" ]; then
          hostname "$NAME"
        fi
      fi
    '';
  };

  # Generate interface rename rules (.link files) from HCL config at boot
  systemd.services.nifty-link = {
    description = "Generate interface rename rules from HCL config";
    wantedBy = [ "multi-user.target" ];
    after = [ "nifty-filter-init.service" "local-fs.target" ];
    before = [ "nifty-network.service" "nifty-filter.service" ];
    serviceConfig = {
      Type = "oneshot";
      RemainAfterExit = true;
    };
    path = [ pkgs.systemd ];
    script = ''
      if [ ! -f ${hclFile} ]; then
        echo "No HCL config found, skipping link generation"
        exit 0
      fi
      mkdir -p /run/systemd/network
      ${nifty-filter}/bin/nifty-filter generate linkfiles --config ${hclFile} --output-dir /run/systemd/network
      udevadm trigger --subsystem-match=net --action=add
      udevadm settle
    '';
  };
}
