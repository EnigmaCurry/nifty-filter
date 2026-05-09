<script lang="ts">
  import { onMount } from "svelte";
  import * as Card from "$lib/components/ui/card/index.js";

  interface UptimeInfo {
    uptime_seconds: number;
  }

  interface NetworkInterface {
    name: string;
    mtu?: number;
    state: string;
    mac?: string;
    addresses: string[];
    link_kind?: string;
  }

  interface NftChain {
    family: string;
    table: string;
    name: string;
    chain_type?: string;
    hook?: string;
    priority?: number;
    policy?: string;
  }

  interface PortStats {
    port: number;
    enabled: boolean;
    link_up: boolean;
    tx_good: number;
    tx_bad: number;
    rx_good: number;
    rx_bad: number;
  }

  interface VlanEntry {
    vid: number;
    name: string;
    member_ports: string;
    tagged_ports: string;
    untagged_ports: string;
  }

  interface PortVlanSetting {
    port: number;
    pvid: number;
    accepted_frame_type: string;
  }

  interface SwitchInfo {
    device_type: string;
    mac_address: string;
    ip_address: string;
    netmask: string;
    gateway: string;
    firmware_version: string;
    firmware_date: string;
    hardware_version: string;
  }

  interface SwitchState {
    timestamp: number;
    info: SwitchInfo;
    stats: PortStats[];
    vlans: VlanEntry[];
    pvid: PortVlanSetting[];
  }

  interface StatusData {
    uptime: UptimeInfo | null;
    interfaces: NetworkInterface[];
    nft_chains: NftChain[];
    switch: SwitchState | null;
  }

  interface ConfigEntry {
    key: string;
    value: string;
    comment?: string;
    is_commented_out: boolean;
    boot_value?: string;
    is_default?: boolean;
  }

  interface ConfigSection {
    name: string;
    entries: ConfigEntry[];
  }

  interface CakeTin {
    name: string;
    threshold: string;
    target: string;
    packets: number;
    bytes: number;
    drops: number;
    marks: number;
    peak_delay: string;
    avg_delay: string;
    backlog: string;
    sp_flows: number;
    bk_flows: number;
  }

  interface CakeStats {
    device: string;
    bandwidth: string;
    sent_bytes: number;
    sent_packets: number;
    dropped: number;
    overlimits: number;
    tins: CakeTin[];
  }

  interface VlanQosClass {
    vlan_id: string;
    name: string;
    qos_class: string;
  }

  interface QosOverrideEntry {
    class: string;
    cidrs: string;
  }

  interface QosConfigInfo {
    upload_mbps: string;
    download_mbps: string;
    shave_percent: string;
    effective_upload_kbit: number;
    effective_download_kbit: number;
    wan_interface: string;
    vlan_classes: VlanQosClass[];
    overrides: QosOverrideEntry[];
  }

  interface DscpRule {
    text: string;
    description?: string;
  }

  interface QosData {
    configured: boolean;
    active: boolean;
    config: QosConfigInfo | null;
    upload: CakeStats | null;
    download: CakeStats | null;
    dscp_rules: DscpRule[];
  }

  interface UpdatesData {
    nixos_version: string | null;
    nixpkgs_date: string | null;
    nixpkgs_age_seconds: number | null;
    built_at: string | null;
    built_age_seconds: number | null;
    nifty_filter_version: string | null;
    kernel_version: string | null;
  }

  type Tab = "config" | "interfaces" | "nftables" | "qos" | "switch" | "updates" | "about";

  interface AboutData {
    version: string;
    repository: string;
    license: string;
  }

  type ConfigSubTab = "overview" | "environment";

  let data = $state<StatusData | null>(null);
  let configData = $state<ConfigSection[]>([]);
  let rebootNeeded = $state(false);
  let aboutData = $state<AboutData | null>(null);
  let qosData = $state<QosData | null>(null);
  let updatesData = $state<UpdatesData | null>(null);
  let loading = $state(true);
  let errorMsg = $state("");
  let activeTab = $state<Tab>("config");
  let configSubTab = $state<ConfigSubTab>("overview");

  function updateHash() {
    let hash: string;
    if (activeTab === "nftables") {
      hash = `#nftables/${activeHook}`;
    } else if (activeTab === "config") {
      hash = `#config/${configSubTab}`;
    } else {
      hash = `#${activeTab}`;
    }
    history.pushState(null, "", hash);
  }

  function readHash(): { tab: Tab; hook: string; configSub: ConfigSubTab } | null {
    const hash = window.location.hash.slice(1);
    if (!hash) return null;
    const [tab, sub] = hash.split("/");
    const validTabs: Tab[] = ["config", "interfaces", "nftables", "qos", "switch", "updates", "about"];
    if (validTabs.includes(tab as Tab)) {
      return {
        tab: tab as Tab,
        hook: tab === "nftables" ? (sub ?? "input") : "input",
        configSub: tab === "config" && sub === "environment" ? "environment" : "overview",
      };
    }
    return null;
  }

  function cfgVal(key: string): string {
    for (const section of configData) {
      for (const entry of section.entries) {
        if (entry.key === key && !entry.is_commented_out) return entry.value;
      }
    }
    return "";
  }

  interface VlanOverview {
    id: string;
    name: string;
    subnet_ipv4: string;
    subnet_ipv6: string;
    egress_ipv4: string;
    egress_ipv6: string;
    tcp_accept: string;
    udp_accept: string;
    dhcp: boolean;
    dhcpv6: boolean;
    iperf: boolean;
  }

  const VALID_STATIC_KEYS = new Set([
    "ENABLED", "HOSTNAME",
    "TRUNK_INTERFACE", "LAN_INTERFACE", "WAN_INTERFACE", "MGMT_INTERFACE",
    "MGMT_SUBNET",
    "WAN_MAC", "TRUNK_MAC", "MGMT_MAC",
    "VLAN_AWARE_SWITCH", "VLANS",
    "WAN_ENABLE_IPV4", "WAN_ENABLE_IPV6",
    "ENABLE_IPV4", "ENABLE_IPV6",
    "WAN_ICMP_ACCEPT", "WAN_ICMPV6_ACCEPT",
    "WAN_TCP_ACCEPT", "WAN_UDP_ACCEPT",
    "WAN_TCP_FORWARD", "WAN_UDP_FORWARD",
    "WAN_BOGONS_IPV4", "WAN_BOGONS_IPV6",
    "WAN_QOS_UPLOAD_MBPS", "WAN_QOS_DOWNLOAD_MBPS", "WAN_QOS_SHAVE_PERCENT",
    "QOS_OVERRIDE_VOICE", "QOS_OVERRIDE_VIDEO", "QOS_OVERRIDE_BESTEFFORT", "QOS_OVERRIDE_BULK",
    "IPERF_PORT",
    "DHCP_UPSTREAM_DNS",
    // Legacy VLAN 1 aliases
    "SUBNET_LAN_IPV4", "SUBNET_LAN", "SUBNET_LAN_IPV6",
    "LAN_EGRESS_ALLOWED_IPV4", "LAN_EGRESS_ALLOWED_IPV6",
    "ICMP_ACCEPT_LAN", "ICMPV6_ACCEPT_LAN",
    "TCP_ACCEPT_LAN", "UDP_ACCEPT_LAN",
    "TCP_FORWARD_LAN", "UDP_FORWARD_LAN",
    "DHCP_POOL_START", "DHCP_POOL_END", "DHCP_ROUTER", "DHCP_DNS",
    "DHCPV6_POOL_START", "DHCPV6_POOL_END",
    "IPERF_ENABLED", "DHCP4_ENABLED", "DHCPV6_ENABLED",
    // Sodola switch
    "SODOLA_URL", "SODOLA_USER", "SODOLA_PASS",
    "SODOLA_MGMT_IFACE", "SODOLA_ROUTER_IP",
    "SODOLA_SWITCH_CONFIG", "SODOLA_STATE_FILE", "SODOLA_INTERVAL", "SODOLA_CONFIG_DIR",
  ]);

  const VALID_VLAN_SUFFIXES = new Set([
    "NAME", "SUBNET_IPV4", "SUBNET_IPV6",
    "EGRESS_ALLOWED_IPV4", "EGRESS_ALLOWED_IPV6",
    "TCP_ACCEPT", "UDP_ACCEPT", "ICMP_ACCEPT", "ICMPV6_ACCEPT",
    "TCP_FORWARD", "UDP_FORWARD",
    "ALLOW_INBOUND_TCP", "ALLOW_INBOUND_UDP",
    "DHCP_ENABLED", "DHCP_POOL_START", "DHCP_POOL_END", "DHCP_ROUTER", "DHCP_DNS",
    "DHCPV6_ENABLED", "DHCPV6_POOL_START", "DHCPV6_POOL_END",
    "QOS_CLASS", "IPERF_ENABLED",
  ]);

  const KNOWN_PREFIXES = ["SWITCH_", "NIFTY_DASHBOARD_"];

  function isValidKey(key: string): boolean {
    if (VALID_STATIC_KEYS.has(key)) return true;
    const m = key.match(/^VLAN_(\d+)_(.+)$/);
    if (m) {
      const suffix = m[2];
      if (VALID_VLAN_SUFFIXES.has(suffix)) return true;
      // VLAN_N_ALLOW_FROM_M_TCP / VLAN_N_ALLOW_FROM_M_UDP
      if (/^ALLOW_FROM_\d+_(TCP|UDP)$/.test(suffix)) return true;
    }
    return false;
  }

  function isKnownKey(key: string): boolean {
    return KNOWN_PREFIXES.some(p => key.startsWith(p));
  }

  function groupPrefix(key: string): string {
    const parts = key.split("_");
    if (parts[0] === "VLAN" && parts.length >= 2) {
      return `${parts[0]}_${parts[1]}`;
    }
    return parts[0];
  }

  function groupedEnvEntries(): ConfigSection[] {
    const all: ConfigEntry[] = [];
    for (const section of configData) {
      for (const entry of section.entries) {
        all.push(entry);
      }
    }
    all.sort((a, b) => a.key.localeCompare(b.key));

    const valid: ConfigEntry[] = [];
    const known: ConfigEntry[] = [];
    const invalid: ConfigEntry[] = [];
    for (const entry of all) {
      if (isValidKey(entry.key)) {
        valid.push(entry);
      } else if (isKnownKey(entry.key)) {
        known.push(entry);
      } else {
        invalid.push(entry);
      }
    }

    const groups = new Map<string, ConfigEntry[]>();
    for (const entry of valid) {
      const prefix = groupPrefix(entry.key);
      if (!groups.has(prefix)) groups.set(prefix, []);
      groups.get(prefix)!.push(entry);
    }
    const general: ConfigEntry[] = [];
    const result: ConfigSection[] = [];
    for (const [name, entries] of groups) {
      if (entries.length === 1) {
        general.push(...entries);
      } else {
        result.push({ name, entries });
      }
    }
    if (general.length > 0) {
      result.unshift({ name: "General", entries: general });
    }

    // Group known (non-nifty-filter) vars by prefix
    const knownGroups = new Map<string, ConfigEntry[]>();
    for (const entry of known) {
      const prefix = groupPrefix(entry.key);
      if (!knownGroups.has(prefix)) knownGroups.set(prefix, []);
      knownGroups.get(prefix)!.push(entry);
    }
    for (const [name, entries] of knownGroups) {
      result.push({ name, entries });
    }

    if (invalid.length > 0) {
      result.push({ name: "Invalid", entries: invalid });
    }
    return result;
  }

  function getVlanOverviews(): VlanOverview[] {
    const vlansStr = cfgVal("VLANS");
    if (!vlansStr) return [];
    return vlansStr.split(",").map((id) => {
      const v = id.trim();
      return {
        id: v,
        name: cfgVal(`VLAN_${v}_NAME`) || `VLAN ${v}`,
        subnet_ipv4: cfgVal(`VLAN_${v}_SUBNET_IPV4`),
        subnet_ipv6: cfgVal(`VLAN_${v}_SUBNET_IPV6`),
        egress_ipv4: cfgVal(`VLAN_${v}_EGRESS_ALLOWED_IPV4`),
        egress_ipv6: cfgVal(`VLAN_${v}_EGRESS_ALLOWED_IPV6`),
        tcp_accept: cfgVal(`VLAN_${v}_TCP_ACCEPT`),
        udp_accept: cfgVal(`VLAN_${v}_UDP_ACCEPT`),
        dhcp: cfgVal(`VLAN_${v}_DHCP_ENABLED`) === "true",
        dhcpv6: cfgVal(`VLAN_${v}_DHCPV6_ENABLED`) === "true",
        iperf: cfgVal(`VLAN_${v}_IPERF_ENABLED`) === "true",
      };
    });
  }

  interface NftRule {
    text: string;
    description?: string;
    in_source: boolean;
  }

  // nft state
  let activeHook = $state("input");
  let chainRulesMap = $state<Record<string, NftRule[]>>({});
  let rulesLoading = $state(false);

  const hookOrder = ["input", "forward", "output", "prerouting", "postrouting"];

  const hookDescriptions: Record<string, string> = {
    input: "Filter incoming traffic destined for the router itself.",
    forward: "Filter traffic passing through the router between networks.",
    output: "Filter traffic originating from the router itself.",
    prerouting: "Rewrite destination addresses before routing (DNAT/port forwarding).",
    postrouting: "Rewrite source addresses after routing (SNAT/masquerade).",
  };

  function availableHooks(): string[] {
    if (!data) return [];
    const hooks = new Set(data.nft_chains.map((c) => c.hook).filter((h): h is string => h != null));
    return hookOrder.filter((h) => hooks.has(h));
  }

  function chainsForHook(hook: string): NftChain[] {
    if (!data) return [];
    return data.nft_chains
      .filter((c) => c.hook === hook)
      .sort((a, b) => (a.priority ?? 0) - (b.priority ?? 0));
  }

  function jumpChainsForHook(hook: string): NftChain[] {
    if (!data) return [];
    return data.nft_chains
      .filter((c) => c.hook == null && c.name.startsWith(`${hook}_`));
  }

  const tabs: { id: Tab; label: string; condition: () => boolean }[] = [
    { id: "config", label: "Config", condition: () => configData.length > 0 },
    { id: "interfaces", label: "Interfaces", condition: () => (data?.interfaces.length ?? 0) > 0 },
    { id: "nftables", label: "Netfilter", condition: () => (data?.nft_chains.length ?? 0) > 0 },
    { id: "qos", label: "QoS", condition: () => qosData != null },
    { id: "switch", label: "Switch", condition: () => data?.switch != null },
    { id: "updates", label: "Updates", condition: () => updatesData != null },
    { id: "about", label: "About", condition: () => aboutData != null },
  ];

  function formatUptime(seconds: number): string {
    const days = Math.floor(seconds / 86400);
    const hours = Math.floor((seconds % 86400) / 3600);
    const mins = Math.floor((seconds % 3600) / 60);
    const parts: string[] = [];
    if (days > 0) parts.push(`${days}d`);
    if (hours > 0) parts.push(`${hours}h`);
    parts.push(`${mins}m`);
    return parts.join(" ");
  }

  function formatTimestamp(ts: number): string {
    return new Date(ts * 1000).toLocaleString();
  }

  function stateColor(state: string): string {
    const s = state.toUpperCase();
    if (s === "UP") return "text-green-400";
    if (s === "DOWN") return "text-red-400";
    if (s === "LOWERLAYERDOWN") return "text-red-400";
    return "text-yellow-400";
  }

  function linkColor(up: boolean): string {
    return up ? "text-green-400" : "text-zinc-500";
  }

  function policyColor(policy: string): string {
    if (policy === "accept") return "text-green-400";
    if (policy === "drop") return "text-red-400";
    return "text-yellow-400";
  }

  function escapeHtml(s: string): string {
    return s.replace(/&/g, "&amp;").replace(/</g, "&lt;").replace(/>/g, "&gt;").replace(/"/g, "&quot;");
  }

  function jumpRuleCount(chainName: string): string {
    // Look up the chain in all nft_chains to build its key, then check chainRulesMap
    const chain = data?.nft_chains.find((c) => c.name === chainName);
    if (!chain) return "";
    const rules = chainRulesMap[chainKey(chain)];
    if (!rules || rules.length === 0) return "";
    return ` <span class="text-muted-foreground text-xs">(${rules.length} rule${rules.length === 1 ? "" : "s"})</span>`;
  }

  function highlightRule(rule: string): string {
    const escaped = escapeHtml(rule);
    return escaped
      // Verdicts
      .replace(/\b(accept)\b/g, '<span class="text-green-400">$1</span>')
      .replace(/\b(drop)\b/g, '<span class="text-red-400">$1</span>')
      .replace(/\b(reject)\b/g, '<span class="text-red-400">$1</span>')
      .replace(/\b(jump|goto)\s+(\S+)/g, (_, verb, target) =>
        `<span class="text-blue-400">${verb}</span> <a href="#chain-${target}" class="text-blue-300 underline decoration-blue-300/30 hover:decoration-blue-300">${target}</a>${jumpRuleCount(target)}`
      )
      .replace(/\b(return)\b/g, '<span class="text-blue-400">$1</span>')
      // Interface matches
      .replace(/\b(iif|oif)\s+(!=\s+)?(&quot;[^&]*&quot;)/g, '<span class="text-purple-400">$1</span> $2<span class="text-purple-300">$3</span>')
      // IP addresses/subnets (IPv4)
      .replace(/\b(\d{1,3}\.\d{1,3}\.\d{1,3}\.\d{1,3}(?:\/\d{1,2})?)\b/g, '<span class="text-cyan-400">$1</span>')
      // IPv6 addresses
      .replace(/((?:[0-9a-fA-F]{0,4}:){2,7}[0-9a-fA-F]{0,4}(?:\/\d{1,3})?)/g, '<span class="text-cyan-400">$1</span>')
      // Port numbers after dport/sport
      .replace(/\b(dport|sport)\s+(\d+)\b/g, '$1 <span class="text-amber-300">$2</span>')
      // Set braces and contents
      .replace(/(\{[^}]+\})/g, '<span class="text-amber-300">$1</span>')
      // Protocol/field keywords
      .replace(/\b(tcp|udp|icmp|icmpv6|ct state|ct status|meta nfproto|ip saddr|ip daddr|ip6 saddr|ip6 daddr|tcp dport|udp dport|udp sport|icmp type|icmpv6 type)\b/g, '<span class="text-sky-400">$1</span>')
      // Log prefix strings
      .replace(/(log prefix &quot;[^&]*&quot;)/g, '<span class="text-zinc-500">$1</span>');
  }

  function chainKey(c: NftChain): string {
    return `${c.family}:${c.table}:${c.name}`;
  }

  async function fetchRulesForChain(chain: NftChain): Promise<NftRule[]> {
    try {
      const params = new URLSearchParams({
        family: chain.family,
        table: chain.table,
        chain: chain.name,
      });
      const res = await fetch(`/api/status/nft-rules?${params}`, { credentials: "include" });
      if (res.ok) {
        const body = await res.json();
        return body.data?.rules ?? [];
      }
    } catch {}
    return [];
  }

  async function selectHook(hook: string) {
    activeHook = hook;
    updateHash();
    rulesLoading = true;
    chainRulesMap = {};
    const allChains = [...chainsForHook(hook), ...jumpChainsForHook(hook)];
    const results = await Promise.all(
      allChains.map(async (c) => ({ key: chainKey(c), rules: await fetchRulesForChain(c) }))
    );
    const map: Record<string, string[]> = {};
    for (const r of results) map[r.key] = r.rules;
    chainRulesMap = map;
    rulesLoading = false;
  }

  async function fetchStatus() {
    try {
      const res = await fetch("/api/status", { credentials: "include" });
      if (!res.ok) {
        errorMsg = `HTTP ${res.status}`;
        return;
      }
      const body = await res.json();
      const isFirstLoad = data == null;
      data = body.data;
      errorMsg = "";

      if (isFirstLoad && data && data.nft_chains.length > 0) {
        const saved = readHash();
        if (saved) {
          activeTab = saved.tab;
          configSubTab = saved.configSub;
          if (saved.tab === "nftables") {
            selectHook(saved.hook);
          } else {
            selectHook("input");
          }
        } else {
          selectHook("input");
          updateHash();
        }
      }
    } catch (e) {
      errorMsg = String(e);
    } finally {
      loading = false;
    }
  }

  async function fetchConfig() {
    try {
      const res = await fetch("/api/status/config", { credentials: "include" });
      if (res.ok) {
        const body = await res.json();
        configData = body.data?.sections ?? [];
        rebootNeeded = body.data?.reboot_needed ?? false;
      }
    } catch {}
  }

  function formatBytes(bytes: number): string {
    if (bytes < 1024) return `${bytes} B`;
    if (bytes < 1048576) return `${(bytes / 1024).toFixed(1)} KiB`;
    if (bytes < 1073741824) return `${(bytes / 1048576).toFixed(1)} MiB`;
    return `${(bytes / 1073741824).toFixed(2)} GiB`;
  }

  function formatKbit(kbit: number): string {
    if (kbit < 1000) return `${kbit} kbit`;
    return `${(kbit / 1000).toFixed(0)} Mbit`;
  }

  function tinColor(name: string): string {
    switch (name) {
      case "Voice": return "text-purple-400";
      case "Video": return "text-blue-400";
      case "Best Effort": return "text-green-400";
      case "Bulk": return "text-zinc-400";
      default: return "";
    }
  }

  async function fetchQos() {
    try {
      const res = await fetch("/api/qos", { credentials: "include" });
      if (res.ok) {
        const body = await res.json();
        qosData = body.data ?? null;
      }
    } catch {}
  }

  async function fetchUpdates() {
    try {
      const res = await fetch("/api/updates", { credentials: "include" });
      if (res.ok) {
        const body = await res.json();
        updatesData = body.data ?? null;
      }
    } catch {}
  }

  async function fetchAbout() {
    try {
      const res = await fetch("/api/status/about", { credentials: "include" });
      if (res.ok) {
        const body = await res.json();
        aboutData = body.data ?? null;
      }
    } catch {}
  }

  onMount(() => {
    fetchConfig();
    fetchAbout();
    fetchQos();
    fetchUpdates();
    fetchStatus();
    const interval = setInterval(() => { fetchStatus(); fetchQos(); }, 15000);

    // SSE: listen for config file changes and re-fetch config in realtime
    const eventSource = new EventSource("/api/events", { withCredentials: true });
    eventSource.addEventListener("config-changed", () => {
      console.log("SSE: config-changed event received, re-fetching config");
      fetchConfig();
    });
    eventSource.onerror = (e) => {
      console.warn("SSE connection error", e);
    };

    function onPopState() {
      const saved = readHash();
      if (saved) {
        activeTab = saved.tab;
        configSubTab = saved.configSub;
        if (saved.tab === "nftables") {
          selectHook(saved.hook);
        }
      }
    }
    window.addEventListener("popstate", onPopState);

    return () => {
      clearInterval(interval);
      eventSource.close();
      window.removeEventListener("popstate", onPopState);
    };
  });
</script>

<svelte:head>
  <title>Status</title>
</svelte:head>

<div class="min-h-screen px-4 py-2 md:px-8 md:py-3 max-w-6xl mx-auto space-y-3">
  <!-- Title bar with uptime -->
  <div class="flex items-baseline justify-between">
    <h1 class="text-3xl font-bold tracking-tight">nifty-filter
      {#if cfgVal("HOSTNAME")}
        <span class="text-lg font-normal text-muted-foreground ml-2">{cfgVal("HOSTNAME")}</span>
      {/if}
    </h1>
    <div class="flex items-baseline gap-3">
      {#if rebootNeeded}
        <span class="text-sm font-semibold text-yellow-400">Reboot needed</span>
      {/if}
      {#if data?.uptime}
        <span class="text-sm text-muted-foreground">
          up <span class="font-mono text-foreground">{formatUptime(data.uptime.uptime_seconds)}</span>
        </span>
      {/if}
    </div>
  </div>

  {#if loading}
    <p class="text-muted-foreground">Loading...</p>
  {:else if errorMsg}
    <Card.Root>
      <Card.Header>
        <Card.Title>Error</Card.Title>
      </Card.Header>
      <Card.Content>
        <p class="text-red-500">{errorMsg}</p>
      </Card.Content>
    </Card.Root>
  {:else if data}
    <!-- Tab bar -->
    <div class="flex gap-1 border-b border-border">
      {#each tabs as tab}
        {#if tab.condition()}
          <button
            class="px-4 py-2 text-sm font-medium transition-colors {activeTab === tab.id
              ? 'border-b-2 border-primary text-foreground'
              : 'text-muted-foreground hover:text-foreground'}"
            onclick={() => { activeTab = tab.id; updateHash(); }}
          >
            {tab.label}
          </button>
        {/if}
      {/each}
    </div>

    <!-- Tab content -->
    <div class="pt-2">
      {#if activeTab === "config"}
        <div class="flex gap-1 border-b border-border/50 mb-4">
          <button
            class="px-3 py-1.5 text-sm font-medium transition-colors {configSubTab === 'overview'
              ? 'border-b-2 border-primary text-foreground'
              : 'text-muted-foreground hover:text-foreground'}"
            onclick={() => { configSubTab = "overview"; updateHash(); }}
          >Overview</button>
          <button
            class="px-3 py-1.5 text-sm font-medium transition-colors {configSubTab === 'environment'
              ? 'border-b-2 border-primary text-foreground'
              : 'text-muted-foreground hover:text-foreground'}"
            onclick={() => { configSubTab = "environment"; updateHash(); }}
          >Environment</button>
        </div>

        {#if configSubTab === "overview"}
          {@const hostname = cfgVal("HOSTNAME") || "nifty-filter"}
          {@const wanIface = cfgVal("WAN_INTERFACE")}
          {@const trunkIface = cfgVal("TRUNK_INTERFACE")}
          {@const mgmtIface = cfgVal("MGMT_INTERFACE")}
          {@const mgmtSubnet = cfgVal("MGMT_SUBNET")}
          {@const ipv4 = cfgVal("WAN_ENABLE_IPV4") === "true"}
          {@const ipv6 = cfgVal("WAN_ENABLE_IPV6") === "true"}
          {@const vlanSwitch = cfgVal("VLAN_AWARE_SWITCH") === "true"}
          {@const vlans = getVlanOverviews()}

          <div class="space-y-4">
            <!-- System -->
            <Card.Root>
              <Card.Content class="pt-2">
                <div class="grid grid-cols-2 md:grid-cols-4 gap-4 text-sm">
                  <div>
                    <span class="text-muted-foreground">Hostname</span>
                    <p class="font-mono font-semibold">{hostname}</p>
                  </div>
                  <div>
                    <span class="text-muted-foreground">WAN Interface</span>
                    <p class="font-mono">{wanIface || "—"}</p>
                  </div>
                  <div>
                    <span class="text-muted-foreground">Trunk Interface</span>
                    <p class="font-mono">{trunkIface || "—"}</p>
                  </div>
                  <div>
                    <span class="text-muted-foreground">Mode</span>
                    <p class="font-mono">{vlanSwitch ? "VLAN-aware switch" : "Simple (VLAN 1 on trunk)"}</p>
                  </div>
                  <div>
                    <span class="text-muted-foreground">WAN IPv4</span>
                    <p class="{ipv4 ? 'text-green-400' : 'text-zinc-500'}">{ipv4 ? "Enabled" : "Disabled"}</p>
                  </div>
                  <div>
                    <span class="text-muted-foreground">WAN IPv6</span>
                    <p class="{ipv6 ? 'text-green-400' : 'text-zinc-500'}">{ipv6 ? "Enabled" : "Disabled"}</p>
                  </div>
                  {#if mgmtIface}
                    <div>
                      <span class="text-muted-foreground">Management</span>
                      <p class="font-mono">{mgmtIface}</p>
                    </div>
                    <div>
                      <span class="text-muted-foreground">Mgmt Subnet</span>
                      <p class="font-mono text-cyan-400">{mgmtSubnet}</p>
                    </div>
                  {/if}
                </div>
              </Card.Content>
            </Card.Root>

            <!-- VLANs -->
            {#if vlans.length > 0}
              <Card.Root>
                <Card.Content class="pt-2">
                  <div class="overflow-x-auto">
                    <table class="w-full text-sm">
                      <thead>
                        <tr class="border-b border-border text-left text-muted-foreground">
                          <th class="py-2 pr-4">VLAN</th>
                          <th class="py-2 pr-4">Name</th>
                          <th class="py-2 pr-4">IPv4 Subnet</th>
                          <th class="py-2 pr-4">IPv6 Subnet</th>
                          <th class="py-2 pr-4">Egress</th>
                          <th class="py-2">Services</th>
                        </tr>
                      </thead>
                      <tbody class="font-mono">
                        {#each vlans as vlan}
                          <tr class="border-b border-border/50">
                            <td class="py-2 pr-4 font-semibold">{vlan.id}</td>
                            <td class="py-2 pr-4 text-purple-400">{vlan.name}</td>
                            <td class="py-2 pr-4 text-cyan-400">{vlan.subnet_ipv4 || "—"}</td>
                            <td class="py-2 pr-4 text-cyan-400">{vlan.subnet_ipv6 || "—"}</td>
                            <td class="py-2 pr-4">
                              {#if vlan.egress_ipv4 === "0.0.0.0/0" || vlan.egress_ipv6 === "::/0"}
                                <span class="text-green-400">All</span>
                              {:else if vlan.egress_ipv4 || vlan.egress_ipv6}
                                <span class="text-yellow-400">Restricted</span>
                              {:else}
                                <span class="text-red-400">None</span>
                              {/if}
                            </td>
                            <td class="py-2 text-xs">
                              {#if vlan.dhcp}<span class="text-green-400 mr-2">DHCPv4</span>{/if}
                              {#if vlan.dhcpv6}<span class="text-green-400 mr-2">DHCPv6</span>{/if}
                              {#if vlan.iperf}<span class="text-green-400 mr-2">iperf3</span>{/if}
                              {#if vlan.tcp_accept}<span class="mr-2"><span class="text-muted-foreground">TCP:</span> <span class="text-amber-300">{vlan.tcp_accept}</span></span>{/if}
                              {#if vlan.udp_accept}<span><span class="text-muted-foreground">UDP:</span> <span class="text-amber-300">{vlan.udp_accept}</span></span>{/if}
                            </td>
                          </tr>
                        {/each}
                      </tbody>
                    </table>
                  </div>
                </Card.Content>
              </Card.Root>
            {/if}
          </div>
        {:else}
          <!-- Environment sub-tab -->
          {@const envGroups = groupedEnvEntries()}
          <p class="text-sm text-muted-foreground mb-4">This page shows the comprehensive set of environment variables that nifty-filter accepts as its configuration. These variables represent your desired state, not necessarily the actual router state. Edit <code class="font-mono text-foreground bg-muted px-1 rounded">/var/nifty-filter/nifty-filter.env</code> and your changes will appear here immediately, but will not be applied until you reboot (or restart services). Any changes made since boot will be shown in <span class="text-orange-400">orange</span> and a <span class="font-semibold text-yellow-400">Reboot needed</span> notice will appear at the top.</p>
          <Card.Root>
            <Card.Content class="pt-2">
              <table class="w-full text-sm" style="table-layout:fixed">
                <colgroup>
                  <col style="width: 18rem;" />
                  <col />
                </colgroup>
                <tbody class="font-mono">
                  {#each envGroups as section, sIdx}
                    {@const isInvalid = section.name === "Invalid"}
                    <tr class="bg-muted/30">
                      <td colspan="2" class="py-2 px-2 font-sans font-semibold text-sm {sIdx > 0 ? 'pt-4' : ''} {isInvalid ? 'text-red-400' : ''}">{section.name}</td>
                    </tr>
                    {#each section.entries as entry}
                      <tr class="{entry.is_default ? 'opacity-30' : (entry.is_commented_out && !entry.boot_value) || (!entry.value && !entry.boot_value && !entry.is_commented_out) ? 'opacity-30' : ''}" title={entry.is_default ? 'Default (not set in config file)' : entry.comment ?? ""}>
                        <td class="py-0.5 pr-2 {isInvalid ? 'text-red-400' : 'text-purple-400'} whitespace-nowrap overflow-hidden text-ellipsis">{entry.key}</td>
                        <td class="py-0.5 break-all">
                          {#if entry.is_default}
                            <span class="text-muted-foreground italic">{entry.value || '""'}</span>
                          {:else if entry.boot_value != null}
                            <span class="line-through text-muted-foreground mr-2">{entry.boot_value || '""'}</span>
                            <span class="text-orange-400">{entry.is_commented_out ? "#" : ""}{entry.value || '""'}</span>
                          {:else if !entry.value}
                            <span class="text-muted-foreground">""</span>
                          {:else}
                            <span class="{entry.value === '******' ? 'text-yellow-400' : 'text-green-400'}">{entry.value}</span>
                          {/if}
                        </td>
                      </tr>
                    {/each}
                  {/each}
                </tbody>
              </table>
            </Card.Content>
          </Card.Root>
        {/if}

      {:else if activeTab === "interfaces" && data.interfaces.length > 0}
        <Card.Root>
          <Card.Content class="pt-2">
            <div class="overflow-x-auto">
              <table class="w-full text-sm">
                <thead>
                  <tr class="border-b border-border text-left text-muted-foreground">
                    <th class="py-2 pr-4">Name</th>
                    <th class="py-2 pr-4">State</th>
                    <th class="py-2 pr-4">MAC</th>
                    <th class="py-2">Addresses</th>
                  </tr>
                </thead>
                <tbody class="font-mono">
                  {#each data.interfaces as iface}
                    <tr class="border-b border-border/50">
                      <td class="py-2 pr-4 font-semibold">{iface.name}</td>
                      <td class="py-2 pr-4">
                        {#if iface.name === "lo"}
                          <span class="text-muted-foreground">-</span>
                        {:else}
                          <span class={stateColor(iface.state)}>{iface.state}</span>
                        {/if}
                      </td>
                      <td class="py-2 pr-4 text-xs">{iface.mac ?? "-"}</td>
                      <td class="py-2">
                        {#each iface.addresses.filter((a) => !a.startsWith("fe80:")) as addr}
                          <div>{addr}</div>
                        {/each}
                      </td>
                    </tr>
                  {/each}
                </tbody>
              </table>
            </div>
          </Card.Content>
        </Card.Root>

      {:else if activeTab === "nftables" && data.nft_chains.length > 0}
        <!-- Hook sub-tabs -->
        <div class="flex gap-1 border-b border-border/50 mb-4">
          {#each availableHooks() as hook}
            <button
              class="px-3 py-1.5 text-sm font-medium transition-colors {activeHook === hook
                ? 'border-b-2 border-primary text-foreground'
                : 'text-muted-foreground hover:text-foreground'}"
              onclick={() => selectHook(hook)}
            >
              {hook}
            </button>
          {/each}
        </div>

        <p class="text-sm text-muted-foreground mb-4">{hookDescriptions[activeHook] ?? ""}</p>

        {#if rulesLoading}
          <p class="text-muted-foreground text-sm">Loading rules...</p>
        {:else}
          {@const hookedChains = chainsForHook(activeHook)}
          {@const jumpChains = jumpChainsForHook(activeHook)}
          {@const allChains = [...hookedChains, ...jumpChains]}
          <Card.Root>
            <Card.Content class="pt-2">
              <div class="overflow-x-auto">
                <table class="w-full text-sm" style="table-layout:fixed">
                  <colgroup>
                    <col style="width: 2.5rem;" />
                    <col />
                    <col style="width: 18rem;" />
                  </colgroup>
                  <thead>
                    <tr class="border-b border-border text-left text-muted-foreground">
                      <th class="py-2 pr-4">#</th>
                      <th class="py-2">Rule</th>
                      <th class="py-2 pl-4">Description</th>
                    </tr>
                  </thead>
                  <tbody class="font-mono">
                    {#each allChains as chain, chainIdx}
                      {@const rules = chainRulesMap[chainKey(chain)] ?? []}
                      {@const isHooked = chain.hook != null}
                      <tr id={isHooked ? undefined : `chain-${chain.name}`} class="bg-muted/30">
                        <td colspan="3" class="py-3 px-2">
                          <div class="flex items-center justify-between">
                            <span class="font-mono font-semibold text-sm">
                              {#if isHooked}
                                {chain.family} {chain.table} {chain.name}
                              {:else}
                                {chain.name}
                              {/if}
                            </span>
                            <div class="flex items-center gap-3 text-xs font-sans">
                              {#if !isHooked && chainIdx === hookedChains.length}
                                <span class="text-muted-foreground italic mr-2">Jump targets</span>
                              {/if}
                              {#if chain.priority != null}
                                <span class="text-muted-foreground">priority {chain.priority}</span>
                              {/if}
                              {#if chain.policy}
                                <span class="text-muted-foreground">policy</span>
                                <span class="font-mono font-semibold {policyColor(chain.policy)}">{chain.policy}</span>
                              {/if}
                            </div>
                          </div>
                        </td>
                      </tr>
                      {#if rules.length === 0}
                        <tr>
                          <td colspan="3" class="py-2 text-muted-foreground text-sm font-sans">No rules in this chain.</td>
                        </tr>
                      {:else}
                        {#each rules as rule, i}
                          <tr class="border-b border-border/50 {rule.in_source ? '' : 'bg-yellow-500/10'}">
                            <td class="py-2 pr-4 text-muted-foreground">{i + 1}</td>
                            <td class="py-2 whitespace-pre-wrap">
                              {#if !rule.in_source}<span class="text-yellow-400 mr-1" title="Not in original config">*</span>{/if}{@html highlightRule(rule.text)}
                            </td>
                            <td class="py-2 pl-4 text-muted-foreground text-xs font-sans">{rule.description ?? ""}</td>
                          </tr>
                        {/each}
                      {/if}
                    {/each}
                  </tbody>
                </table>
              </div>
            </Card.Content>
          </Card.Root>
        {/if}

      {:else if activeTab === "qos" && qosData}
        {#if !qosData.configured && !qosData.active}
          <Card.Root>
            <Card.Header>
              <Card.Title>QoS Not Configured</Card.Title>
              <Card.Description>CAKE traffic shaping eliminates bufferbloat and lets you prioritize traffic by VLAN.</Card.Description>
            </Card.Header>
            <Card.Content>
              <div class="space-y-5 text-sm">
                <p class="text-muted-foreground">Before applying QoS, you should run a speedtest of your native ISP performance to determine your peak upload and download speeds. You can use a site like <a href="https://www.speedtest.net" target="_blank" rel="noopener" class="text-blue-400 underline decoration-blue-400/30 hover:decoration-blue-400">speedtest.net</a> or <a href="https://fast.com" target="_blank" rel="noopener" class="text-blue-400 underline decoration-blue-400/30 hover:decoration-blue-400">fast.com</a>. Run the test a few times and use the highest results you see.</p>

                <p class="text-muted-foreground">Next, add these variables to your <code class="font-mono text-foreground bg-muted px-1 rounded">/var/nifty-filter/nifty-filter.env</code> and reboot:</p>

                <pre class="bg-muted/50 border border-border rounded-md p-4 font-mono text-xs overflow-x-auto"><span class="text-muted-foreground"># Your raw speedtest results (both required to enable QoS)</span>
<span class="text-purple-400">WAN_QOS_UPLOAD_MBPS</span>=<span class="text-green-400">20</span>
<span class="text-purple-400">WAN_QOS_DOWNLOAD_MBPS</span>=<span class="text-green-400">300</span>
<span class="text-purple-400">WAN_QOS_SHAVE_PERCENT</span>=<span class="text-green-400">10</span>          <span class="text-muted-foreground"># default 10%, applied automatically</span>

<span class="text-muted-foreground"># Per-VLAN upload priority (optional)</span>
<span class="text-muted-foreground"># Values: voice, video, besteffort, bulk</span>
<span class="text-purple-400">VLAN_10_QOS_CLASS</span>=<span class="text-green-400">besteffort</span>
<span class="text-purple-400">VLAN_20_QOS_CLASS</span>=<span class="text-green-400">bulk</span></pre>

                <div class="space-y-4">
                  <div>
                    <h4 class="font-semibold mb-2">How it works</h4>
                    <div class="text-muted-foreground space-y-2 text-xs">
                      <p>Bufferbloat happens when your ISP's modem has large internal buffers that fill up during heavy traffic. Once those buffers are full, every packet — including time-sensitive things like video calls and gaming — has to wait in line behind bulk downloads. This causes latency to spike from a few milliseconds to hundreds or even thousands of milliseconds.</p>
                      <p>CAKE (Common Applications Kept Enhanced) is a queue discipline that solves this by rate-limiting your traffic slightly below your actual link speed. This keeps the bottleneck on your router instead of your ISP's modem. Since CAKE controls the queue, it can use smart scheduling to keep latency low even when the link is fully loaded.</p>
                      <p>The shave factor (default 10%) is how much below your raw speed CAKE will target. For example, if your upload is 20 Mbps, CAKE will shape to 18 Mbps. This small reduction is what prevents your ISP's buffers from ever filling up. If your connection speed is very stable (like fiber), you can reduce the shave to 5%. If it fluctuates (like cable or DSL), keep it at 10% or higher.</p>
                      <p>You can optionally assign each VLAN a traffic class. CAKE sorts traffic into four priority tins based on DSCP markings. Higher-priority tins get preferential scheduling during congestion, so a VoIP call on a "voice" VLAN won't be disrupted by a large download on a "bulk" VLAN. Traffic without an explicit class defaults to best effort.</p>
                    </div>
                  </div>

                  <div>
                    <h4 class="font-semibold mb-2">CAKE priority tins</h4>
                    <table class="w-full font-mono text-xs">
                      <tbody>
                        <tr><td class="py-0.5 pr-3 text-purple-400">voice</td><td class="text-muted-foreground">Highest priority. VoIP, real-time audio. (DSCP EF)</td></tr>
                        <tr><td class="py-0.5 pr-3 text-blue-400">video</td><td class="text-muted-foreground">Video calls, streaming, cameras. (DSCP AF41)</td></tr>
                        <tr><td class="py-0.5 pr-3 text-green-400">besteffort</td><td class="text-muted-foreground">General web traffic. This is the default. (DSCP CS0)</td></tr>
                        <tr><td class="py-0.5 pr-3 text-zinc-400">bulk</td><td class="text-muted-foreground">Lowest priority. IoT, backups, large downloads. (DSCP CS1)</td></tr>
                      </tbody>
                    </table>
                  </div>
                </div>
              </div>
            </Card.Content>
          </Card.Root>
        {:else if qosData.configured && !qosData.active}
          <Card.Root>
            <Card.Header>
              <Card.Title>QoS Configured — Reboot Required</Card.Title>
              <Card.Description>QoS settings have been added to the configuration but CAKE is not yet active.</Card.Description>
            </Card.Header>
            <Card.Content>
              <div class="space-y-3 text-sm">
                {#if qosData.config}
                  <div class="grid grid-cols-2 md:grid-cols-3 gap-4">
                    <div>
                      <span class="text-muted-foreground">Upload</span>
                      <p class="font-mono">{qosData.config.upload_mbps} Mbps</p>
                    </div>
                    <div>
                      <span class="text-muted-foreground">Download</span>
                      <p class="font-mono">{qosData.config.download_mbps} Mbps</p>
                    </div>
                    <div>
                      <span class="text-muted-foreground">Shave</span>
                      <p class="font-mono">{qosData.config.shave_percent}%</p>
                    </div>
                  </div>
                {/if}
                <p class="text-muted-foreground">Reboot the router to apply QoS traffic shaping. The CAKE qdisc will be configured on the WAN interface at startup.</p>
              </div>
            </Card.Content>
          </Card.Root>
        {:else}
        {@const cfg = qosData.config}
        <div class="space-y-4">
          <!-- QoS Configuration -->
          {#if cfg}
            <Card.Root>
              <Card.Header class="pb-2">
                <Card.Title>Configuration</Card.Title>
              </Card.Header>
              <Card.Content>
                <div class="grid grid-cols-2 md:grid-cols-3 gap-4 text-sm">
                  <div>
                    <span class="text-muted-foreground">WAN Interface</span>
                    <p class="font-mono font-semibold">{cfg.wan_interface}</p>
                  </div>
                  <div>
                    <span class="text-muted-foreground">Raw Upload</span>
                    <p class="font-mono">{cfg.upload_mbps} Mbps</p>
                  </div>
                  <div>
                    <span class="text-muted-foreground">Raw Download</span>
                    <p class="font-mono">{cfg.download_mbps} Mbps</p>
                  </div>
                  <div>
                    <span class="text-muted-foreground">Shave</span>
                    <p class="font-mono">{cfg.shave_percent}%</p>
                  </div>
                  <div>
                    <span class="text-muted-foreground">Effective Upload</span>
                    <p class="font-mono text-green-400">{formatKbit(cfg.effective_upload_kbit)}</p>
                  </div>
                  <div>
                    <span class="text-muted-foreground">Effective Download</span>
                    <p class="font-mono text-green-400">{formatKbit(cfg.effective_download_kbit)}</p>
                  </div>
                </div>

                {#if cfg.vlan_classes.length > 0}
                  <div class="pt-3 border-t border-border/50">
                    <h4 class="text-xs text-muted-foreground font-semibold mb-2">Per-VLAN Upload Priority</h4>
                    <div class="overflow-x-auto">
                      <table class="w-full text-sm">
                        <thead>
                          <tr class="border-b border-border text-left text-muted-foreground">
                            <th class="py-1 pr-4">VLAN</th>
                            <th class="py-1 pr-4">Name</th>
                            <th class="py-1">Class</th>
                          </tr>
                        </thead>
                        <tbody class="font-mono">
                          {#each cfg.vlan_classes as vc}
                            <tr class="border-b border-border/50">
                              <td class="py-1 pr-4 font-semibold">{vc.vlan_id}</td>
                              <td class="py-1 pr-4 text-purple-400">{vc.name}</td>
                              <td class="py-1 {tinColor(vc.qos_class === 'besteffort' ? 'Best Effort' : vc.qos_class.charAt(0).toUpperCase() + vc.qos_class.slice(1))}">{vc.qos_class}</td>
                            </tr>
                          {/each}
                        </tbody>
                      </table>
                    </div>
                  </div>
                {/if}

                {#if cfg.overrides.length > 0}
                  <div class="pt-3 border-t border-border/50">
                    <h4 class="text-xs text-muted-foreground font-semibold mb-2">Per-CIDR Overrides</h4>
                    <div class="overflow-x-auto">
                      <table class="w-full text-sm">
                        <thead>
                          <tr class="border-b border-border text-left text-muted-foreground">
                            <th class="py-1 pr-4">Class</th>
                            <th class="py-1">CIDRs</th>
                          </tr>
                        </thead>
                        <tbody class="font-mono">
                          {#each cfg.overrides as ovr}
                            <tr class="border-b border-border/50">
                              <td class="py-1 pr-4 {tinColor(ovr.class === 'besteffort' ? 'Best Effort' : ovr.class.charAt(0).toUpperCase() + ovr.class.slice(1))}">{ovr.class}</td>
                              <td class="py-1 text-cyan-400">{ovr.cidrs}</td>
                            </tr>
                          {/each}
                        </tbody>
                      </table>
                    </div>
                  </div>
                {/if}
              </Card.Content>
            </Card.Root>
          {/if}

          <!-- CAKE Stats -->
          {#each [{ label: "Upload", stats: qosData.upload, direction: "egress" }, { label: "Download", stats: qosData.download, direction: "ingress" }] as side}
            {#if side.stats}
              <Card.Root>
                <Card.Header class="pb-2">
                  <Card.Title>{side.label} — {side.stats.device}</Card.Title>
                  <Card.Description>
                    CAKE {side.stats.bandwidth} &middot;
                    {side.stats.sent_packets.toLocaleString()} pkts ({formatBytes(side.stats.sent_bytes)}) &middot;
                    {side.stats.dropped} dropped &middot;
                    {side.stats.overlimits} overlimits
                  </Card.Description>
                </Card.Header>
                <Card.Content>
                  <div class="overflow-x-auto">
                    <table class="w-full text-sm">
                      <thead>
                        <tr class="border-b border-border text-left text-muted-foreground">
                          <th class="py-2 pr-4">Tin</th>
                          <th class="py-2 pr-4">Threshold</th>
                          <th class="py-2 pr-4">Packets</th>
                          <th class="py-2 pr-4">Bytes</th>
                          <th class="py-2 pr-4">Drops</th>
                          <th class="py-2 pr-4">Marks</th>
                          <th class="py-2 pr-4">Peak Delay</th>
                          <th class="py-2 pr-4">Avg Delay</th>
                          <th class="py-2">Flows</th>
                        </tr>
                      </thead>
                      <tbody class="font-mono">
                        {#each side.stats.tins as tin}
                          <tr class="border-b border-border/50">
                            <td class="py-2 pr-4 font-semibold {tinColor(tin.name)}">{tin.name}</td>
                            <td class="py-2 pr-4">{tin.threshold}</td>
                            <td class="py-2 pr-4">{tin.packets.toLocaleString()}</td>
                            <td class="py-2 pr-4">{formatBytes(tin.bytes)}</td>
                            <td class="py-2 pr-4 {tin.drops > 0 ? 'text-red-400' : ''}">{tin.drops}</td>
                            <td class="py-2 pr-4 {tin.marks > 0 ? 'text-yellow-400' : ''}">{tin.marks}</td>
                            <td class="py-2 pr-4">{tin.peak_delay}</td>
                            <td class="py-2 pr-4">{tin.avg_delay}</td>
                            <td class="py-2">{tin.sp_flows + tin.bk_flows}</td>
                          </tr>
                        {/each}
                      </tbody>
                    </table>
                  </div>
                </Card.Content>
              </Card.Root>
            {/if}
          {/each}

          <!-- DSCP Rules -->
          {#if qosData.dscp_rules.length > 0}
            <Card.Root>
              <Card.Header class="pb-2">
                <Card.Title>DSCP Marking Rules</Card.Title>
                <Card.Description>Active nftables mangle rules for upload traffic prioritization</Card.Description>
              </Card.Header>
              <Card.Content>
                <div class="overflow-x-auto">
                  <table class="w-full text-sm" style="table-layout:fixed">
                    <colgroup>
                      <col />
                      <col style="width: 16rem;" />
                    </colgroup>
                    <thead>
                      <tr class="border-b border-border text-left text-muted-foreground">
                        <th class="py-2">Rule</th>
                        <th class="py-2 pl-4">Description</th>
                      </tr>
                    </thead>
                    <tbody class="font-mono">
                      {#each qosData.dscp_rules as rule}
                        <tr class="border-b border-border/50">
                          <td class="py-2 whitespace-pre-wrap">{@html highlightRule(rule.text)}</td>
                          <td class="py-2 pl-4 text-muted-foreground text-xs font-sans">{rule.description ?? ""}</td>
                        </tr>
                      {/each}
                    </tbody>
                  </table>
                </div>
              </Card.Content>
            </Card.Root>
          {/if}

          <!-- How it works -->
          <Card.Root>
            <Card.Header class="pb-2">
              <Card.Title>How it works</Card.Title>
            </Card.Header>
            <Card.Content>
              <div class="space-y-4">
                <div class="text-muted-foreground space-y-2 text-xs">
                  <p>Bufferbloat happens when your ISP's modem has large internal buffers that fill up during heavy traffic. Once those buffers are full, every packet — including time-sensitive things like video calls and gaming — has to wait in line behind bulk downloads. This causes latency to spike from a few milliseconds to hundreds or even thousands of milliseconds.</p>
                  <p>CAKE (Common Applications Kept Enhanced) is a queue discipline that solves this by rate-limiting your traffic slightly below your actual link speed. This keeps the bottleneck on your router instead of your ISP's modem. Since CAKE controls the queue, it can use smart scheduling to keep latency low even when the link is fully loaded.</p>
                  <p>The shave factor (default 10%) is how much below your raw speed CAKE will target. For example, if your upload is 20 Mbps, CAKE will shape to 18 Mbps. This small reduction is what prevents your ISP's buffers from ever filling up. If your connection speed is very stable (like fiber), you can reduce the shave to 5%. If it fluctuates (like cable or DSL), keep it at 10% or higher.</p>
                  <p>You can optionally assign each VLAN a traffic class. CAKE sorts traffic into four priority tins based on DSCP markings. Higher-priority tins get preferential scheduling during congestion, so a VoIP call on a "voice" VLAN won't be disrupted by a large download on a "bulk" VLAN. Traffic without an explicit class defaults to best effort.</p>
                </div>

                <div>
                  <h4 class="text-xs font-semibold mb-2">CAKE priority tins</h4>
                  <table class="w-full font-mono text-xs">
                    <tbody>
                      <tr><td class="py-0.5 pr-3 text-purple-400">voice</td><td class="text-muted-foreground">Highest priority. VoIP, real-time audio. (DSCP EF)</td></tr>
                      <tr><td class="py-0.5 pr-3 text-blue-400">video</td><td class="text-muted-foreground">Video calls, streaming, cameras. (DSCP AF41)</td></tr>
                      <tr><td class="py-0.5 pr-3 text-green-400">besteffort</td><td class="text-muted-foreground">General web traffic. This is the default. (DSCP CS0)</td></tr>
                      <tr><td class="py-0.5 pr-3 text-zinc-400">bulk</td><td class="text-muted-foreground">Lowest priority. IoT, backups, large downloads. (DSCP CS1)</td></tr>
                    </tbody>
                  </table>
                </div>
              </div>
            </Card.Content>
          </Card.Root>
        </div>
        {/if}

      {:else if activeTab === "switch" && data.switch}
        <Card.Root>
          <Card.Header>
            <Card.Title>{data.switch.info.device_type}</Card.Title>
            <Card.Description>
              {data.switch.info.ip_address} &middot; FW {data.switch.info.firmware_version} &middot;
              Last polled {formatTimestamp(data.switch.timestamp)}
            </Card.Description>
          </Card.Header>
          <Card.Content class="space-y-6">
            <!-- Switch Info -->
            <div class="grid grid-cols-2 md:grid-cols-4 gap-3 text-sm">
              <div>
                <span class="text-muted-foreground">MAC</span>
                <p class="font-mono">{data.switch.info.mac_address}</p>
              </div>
              <div>
                <span class="text-muted-foreground">Netmask</span>
                <p class="font-mono">{data.switch.info.netmask}</p>
              </div>
              <div>
                <span class="text-muted-foreground">Gateway</span>
                <p class="font-mono">{data.switch.info.gateway}</p>
              </div>
              <div>
                <span class="text-muted-foreground">Hardware</span>
                <p class="font-mono">{data.switch.info.hardware_version}</p>
              </div>
            </div>

            <!-- Port Stats -->
            <div>
              <h3 class="text-sm font-semibold text-muted-foreground mb-2">Port Statistics</h3>
              <div class="overflow-x-auto">
                <table class="w-full text-sm">
                  <thead>
                    <tr class="border-b border-border text-left text-muted-foreground">
                      <th class="py-2 pr-4">Port</th>
                      <th class="py-2 pr-4">Enabled</th>
                      <th class="py-2 pr-4">Link</th>
                      <th class="py-2 pr-4">TX Good</th>
                      <th class="py-2 pr-4">TX Bad</th>
                      <th class="py-2 pr-4">RX Good</th>
                      <th class="py-2">RX Bad</th>
                    </tr>
                  </thead>
                  <tbody class="font-mono">
                    {#each data.switch.stats as port}
                      <tr class="border-b border-border/50">
                        <td class="py-2 pr-4 font-semibold">{port.port}</td>
                        <td class="py-2 pr-4">
                          <span class={port.enabled ? "text-green-400" : "text-zinc-500"}>
                            {port.enabled ? "Yes" : "No"}
                          </span>
                        </td>
                        <td class="py-2 pr-4">
                          <span class={linkColor(port.link_up)}>
                            {port.link_up ? "Up" : "Down"}
                          </span>
                        </td>
                        <td class="py-2 pr-4">{port.tx_good.toLocaleString()}</td>
                        <td class="py-2 pr-4">{port.tx_bad > 0 ? port.tx_bad.toLocaleString() : "-"}</td>
                        <td class="py-2 pr-4">{port.rx_good.toLocaleString()}</td>
                        <td class="py-2">{port.rx_bad > 0 ? port.rx_bad.toLocaleString() : "-"}</td>
                      </tr>
                    {/each}
                  </tbody>
                </table>
              </div>
            </div>

            <!-- VLANs -->
            <div>
              <h3 class="text-sm font-semibold text-muted-foreground mb-2">VLANs</h3>
              <div class="overflow-x-auto">
                <table class="w-full text-sm">
                  <thead>
                    <tr class="border-b border-border text-left text-muted-foreground">
                      <th class="py-2 pr-4">VID</th>
                      <th class="py-2 pr-4">Name</th>
                      <th class="py-2 pr-4">Members</th>
                      <th class="py-2 pr-4">Tagged</th>
                      <th class="py-2">Untagged</th>
                    </tr>
                  </thead>
                  <tbody class="font-mono">
                    {#each data.switch.vlans as vlan}
                      <tr class="border-b border-border/50">
                        <td class="py-2 pr-4 font-semibold">{vlan.vid}</td>
                        <td class="py-2 pr-4">{vlan.name || "-"}</td>
                        <td class="py-2 pr-4">{vlan.member_ports || "-"}</td>
                        <td class="py-2 pr-4">{vlan.tagged_ports || "-"}</td>
                        <td class="py-2">{vlan.untagged_ports || "-"}</td>
                      </tr>
                    {/each}
                  </tbody>
                </table>
              </div>
            </div>

            <!-- PVID Settings -->
            <div>
              <h3 class="text-sm font-semibold text-muted-foreground mb-2">Port VLAN Settings</h3>
              <div class="overflow-x-auto">
                <table class="w-full text-sm">
                  <thead>
                    <tr class="border-b border-border text-left text-muted-foreground">
                      <th class="py-2 pr-4">Port</th>
                      <th class="py-2 pr-4">PVID</th>
                      <th class="py-2">Accepted Frames</th>
                    </tr>
                  </thead>
                  <tbody class="font-mono">
                    {#each data.switch.pvid as setting}
                      <tr class="border-b border-border/50">
                        <td class="py-2 pr-4 font-semibold">{setting.port}</td>
                        <td class="py-2 pr-4">{setting.pvid}</td>
                        <td class="py-2">{setting.accepted_frame_type}</td>
                      </tr>
                    {/each}
                  </tbody>
                </table>
              </div>
            </div>
          </Card.Content>
        </Card.Root>

      {:else if activeTab === "updates" && updatesData}
        {@const nixpkgsAgeDays = updatesData.nixpkgs_age_seconds != null ? Math.floor(updatesData.nixpkgs_age_seconds / 86400) : null}
        {@const builtAgeDays = updatesData.built_age_seconds != null ? Math.floor(updatesData.built_age_seconds / 86400) : null}
        <div class="space-y-4">
          <Card.Root>
            <Card.Header class="pb-2">
              <Card.Title>System Version</Card.Title>
            </Card.Header>
            <Card.Content>
              <div class="grid grid-cols-2 md:grid-cols-3 gap-4 text-sm">
                {#if updatesData.nixos_version}
                  <div>
                    <span class="text-muted-foreground">NixOS Version</span>
                    <p class="font-mono font-semibold">{updatesData.nixos_version}</p>
                  </div>
                {/if}
                {#if updatesData.kernel_version}
                  <div>
                    <span class="text-muted-foreground">Kernel</span>
                    <p class="font-mono">{updatesData.kernel_version}</p>
                  </div>
                {/if}
                {#if updatesData.nifty_filter_version}
                  <div>
                    <span class="text-muted-foreground">nifty-filter</span>
                    <p class="font-mono">{updatesData.nifty_filter_version}</p>
                  </div>
                {/if}
                {#if updatesData.nixpkgs_date}
                  <div>
                    <span class="text-muted-foreground">Nixpkgs Date</span>
                    <p class="font-mono">
                      {updatesData.nixpkgs_date}
                      {#if nixpkgsAgeDays != null}
                        <span class="text-muted-foreground ml-1">({#if nixpkgsAgeDays === 0}today{:else if nixpkgsAgeDays === 1}1 day ago{:else}{nixpkgsAgeDays} days ago{/if})</span>
                      {/if}
                    </p>
                  </div>
                {/if}
                {#if updatesData.built_at}
                  <div>
                    <span class="text-muted-foreground">Last Built</span>
                    <p class="font-mono">
                      {updatesData.built_at}
                      {#if builtAgeDays != null}
                        <span class="text-muted-foreground ml-1">({#if builtAgeDays === 0}today{:else if builtAgeDays === 1}1 day ago{:else}{builtAgeDays} days ago{/if})</span>
                      {/if}
                    </p>
                  </div>
                {/if}
              </div>
            </Card.Content>
          </Card.Root>

          <Card.Root>
            <Card.Header class="pb-2">
              <Card.Title>How to Update</Card.Title>
            </Card.Header>
            <Card.Content>
              <div class="space-y-4 text-sm">
                <p class="text-muted-foreground">nifty-filter is built from a Nix flake. To update nixpkgs and rebuild the system, run these commands from your workstation in the nifty-filter source directory:</p>

                <div>
                  <h4 class="font-semibold mb-2">1. Update flake inputs</h4>
                  <pre class="bg-muted/50 border border-border rounded-md p-3 font-mono text-xs overflow-x-auto">nix flake update</pre>
                  <p class="text-muted-foreground text-xs mt-1">This updates <code class="font-mono text-foreground bg-muted px-1 rounded">flake.lock</code> to the latest nixpkgs, pulling in new package versions, kernel updates, and security patches.</p>
                </div>

                <div>
                  <h4 class="font-semibold mb-2">2. Commit the lockfile</h4>
                  <pre class="bg-muted/50 border border-border rounded-md p-3 font-mono text-xs overflow-x-auto">git add flake.lock && git commit -m "nix flake update"</pre>
                  <p class="text-muted-foreground text-xs mt-1">The updated lockfile must be committed before building, since Nix flakes require a clean git tree.</p>
                </div>

                <div>
                  <h4 class="font-semibold mb-2">3. Build and deploy</h4>
                  <pre class="bg-muted/50 border border-border rounded-md p-3 font-mono text-xs overflow-x-auto">just pve-upgrade &lt;pve-host&gt; &lt;vmid&gt; &lt;vm-name&gt;</pre>
                  <p class="text-muted-foreground text-xs mt-1">Builds the system closure on your workstation and deploys it to the router via the Proxmox host. The router will reboot into the new system.</p>
                </div>

                <div>
                  <h4 class="font-semibold mb-2">Alternative: build on the router</h4>
                  <pre class="bg-muted/50 border border-border rounded-md p-3 font-mono text-xs overflow-x-auto">sudo nifty-upgrade</pre>
                  <p class="text-muted-foreground text-xs mt-1">Pulls the latest source and builds directly on the router. Requires sufficient RAM and disk space.</p>
                </div>
              </div>
            </Card.Content>
          </Card.Root>
        </div>

      {:else if activeTab === "about" && aboutData}
        <Card.Root>
          <Card.Content class="pt-2">
            <div class="space-y-3 text-sm">
              <div class="flex items-center gap-4">
                <span class="font-mono text-lg font-bold">nifty-filter</span>
                <span class="font-mono text-muted-foreground">v{aboutData.version}</span>
              </div>
              <p>
                <a href={aboutData.repository} target="_blank" rel="noopener" class="text-blue-400 underline decoration-blue-400/30 hover:decoration-blue-400 font-mono text-xs">{aboutData.repository}</a>
              </p>
              <p class="text-muted-foreground">nifty-dashboard is the read-only dashboard you are currently viewing. It presents both the configured state (desired) and the live system state (actual) of your nifty-filter router, so you can see at a glance whether reality matches intent.</p>
              <div>
                <h3 class="text-xs text-muted-foreground mb-1">License (MIT)</h3>
                <textarea readonly class="w-full h-96 bg-muted/30 border border-border rounded-md p-3 font-mono text-xs text-muted-foreground resize-none focus:outline-none">{aboutData.license}</textarea>
              </div>
            </div>
          </Card.Content>
        </Card.Root>
      {/if}
    </div>
  {/if}
</div>
