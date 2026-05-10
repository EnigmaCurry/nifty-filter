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

  // Config is now a generic JSON tree from HCL parsing

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

  interface DnsmasqInterface {
    name: string;
    listen_address: string | null;
    pool_start: string | null;
    pool_end: string | null;
    lease_time: string | null;
    dhcp_router: string | null;
    dhcp_dns: string | null;
    pool_start_v6: string | null;
    pool_end_v6: string | null;
    dhcpv6_dns: string | null;
    ra_enabled: boolean;
  }

  interface DhcpHost {
    mac: string;
    ip: string;
    hostname: string | null;
  }

  interface DhcpLease {
    expires: string;
    mac: string;
    ip: string;
    hostname: string;
    client_id: string;
  }

  interface DnsmasqData {
    config_found: boolean;
    upstream_dns: string[];
    interfaces: DnsmasqInterface[];
    static_hosts: DhcpHost[];
    leases: DhcpLease[];
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

  type Tab = "config" | "state" | "updates" | "about";
  type StateSubTab = "interfaces" | "nftables" | "qos" | "switch" | "dnsmasq";

  interface AboutData {
    version: string;
    repository: string;
    license: string;
  }

  type ConfigSubTab = "overview" | "spec";

  let data = $state<StatusData | null>(null);
  let configJson = $state<Record<string, any> | null>(null);
  let bootConfigJson = $state<Record<string, any> | null>(null);
  let rebootNeeded = $state(false);
  let aboutData = $state<AboutData | null>(null);
  let qosData = $state<QosData | null>(null);
  let dnsmasqData = $state<DnsmasqData | null>(null);
  let updatesData = $state<UpdatesData | null>(null);
  let loading = $state(true);
  let errorMsg = $state("");
  let activeTab = $state<Tab>("config");
  let configSubTab = $state<ConfigSubTab>("overview");
  let stateSubTab = $state<StateSubTab>("interfaces");

  function updateHash() {
    let hash: string;
    if (activeTab === "config") {
      hash = `#config/${configSubTab}`;
    } else if (activeTab === "state") {
      if (stateSubTab === "nftables") {
        hash = `#state/nftables/${activeHook}`;
      } else {
        hash = `#state/${stateSubTab}`;
      }
    } else {
      hash = `#${activeTab}`;
    }
    history.pushState(null, "", hash);
  }

  function readHash(): { tab: Tab; hook: string; configSub: ConfigSubTab; stateSub: StateSubTab } | null {
    const hash = window.location.hash.slice(1);
    if (!hash) return null;
    const parts = hash.split("/");
    const tab = parts[0];
    const validTabs: Tab[] = ["config", "state", "updates", "about"];
    if (validTabs.includes(tab as Tab)) {
      const validStateSubs: StateSubTab[] = ["interfaces", "nftables", "dnsmasq", "qos", "switch"];
      return {
        tab: tab as Tab,
        hook: tab === "state" && parts[1] === "nftables" ? (parts[2] ?? "input") : "input",
        configSub: tab === "config" && parts[1] === "spec" ? "spec" : "overview",
        stateSub: tab === "state" && validStateSubs.includes(parts[1] as StateSubTab) ? parts[1] as StateSubTab : "interfaces",
      };
    }
    return null;
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

  function getVlanOverviews(): VlanOverview[] {
    if (!configJson?.vlan) return [];
    return Object.entries(configJson.vlan as Record<string, any>)
      .sort(([, a], [, b]) => (a.id ?? 0) - (b.id ?? 0))
      .map(([name, v]: [string, any]) => ({
        id: String(v.id ?? ""),
        name,
        subnet_ipv4: v.ipv4?.subnet ?? "",
        subnet_ipv6: v.ipv6?.subnet ?? "",
        egress_ipv4: (v.ipv4?.egress ?? []).join(", "),
        egress_ipv6: (v.ipv6?.egress ?? []).join(", "),
        tcp_accept: (v.firewall?.tcp_accept ?? []).join(", "),
        udp_accept: (v.firewall?.udp_accept ?? []).join(", "),
        dhcp: v.dhcp != null,
        dhcpv6: v.dhcpv6 != null,
        iperf: v.iperf_enabled === true,
      }));
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
    { id: "config", label: "Config", condition: () => true },
    { id: "state", label: "State", condition: () => (data?.interfaces.length ?? 0) > 0 || (data?.nft_chains.length ?? 0) > 0 || dnsmasqData != null || qosData != null || data?.switch != null },
    { id: "updates", label: "Updates", condition: () => updatesData != null },
    { id: "about", label: "About", condition: () => aboutData != null },
  ];

  const stateSubTabs: { id: StateSubTab; label: string; condition: () => boolean }[] = [
    { id: "interfaces", label: "Interfaces", condition: () => (data?.interfaces.length ?? 0) > 0 },
    { id: "nftables", label: "Netfilter", condition: () => (data?.nft_chains.length ?? 0) > 0 },
    { id: "dnsmasq", label: "Dnsmasq", condition: () => dnsmasqData != null },
    { id: "qos", label: "QoS", condition: () => qosData != null },
    { id: "switch", label: "Switch", condition: () => data?.switch != null },
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
          stateSubTab = saved.stateSub;
          if (saved.tab === "state" && saved.stateSub === "nftables") {
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
        configJson = body.data?.config ?? null;
        bootConfigJson = body.data?.boot_config ?? null;
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

  async function fetchDnsmasq() {
    try {
      const res = await fetch("/api/dnsmasq", { credentials: "include" });
      if (res.ok) {
        const body = await res.json();
        dnsmasqData = body.data ?? null;
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
    fetchDnsmasq();
    fetchUpdates();
    fetchStatus();
    const interval = setInterval(() => { fetchStatus(); fetchQos(); fetchDnsmasq(); }, 15000);

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
        stateSubTab = saved.stateSub;
        if (saved.tab === "state" && saved.stateSub === "nftables") {
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

{#snippet specNode(key: string, val: any, bootVal: any, depth: number)}
  {#if val != null && typeof val === "object" && !Array.isArray(val)}
    <!-- Object block: section header + children -->
    <div class="{'ml-' + (depth > 0 ? '4' : '0')} {depth > 0 ? 'border-l border-border/30 pl-3' : ''}">
      <div class="py-1.5 font-sans font-semibold text-sm {depth === 0 ? 'text-foreground border-b border-border/30 mb-1' : 'text-muted-foreground'}">{key}</div>
      {#each Object.entries(val) as [k, v]}
        {@render specNode(k, v, bootVal != null && typeof bootVal === "object" && !Array.isArray(bootVal) ? bootVal[k] : undefined, depth + 1)}
      {/each}
    </div>
  {:else if Array.isArray(val)}
    <!-- Array value -->
    {@const bootArr = Array.isArray(bootVal) ? bootVal : undefined}
    {@const changed = bootArr !== undefined && JSON.stringify(val) !== JSON.stringify(bootArr)}
    <div class="flex py-0.5 {'ml-' + (depth > 0 ? '4' : '0')}">
      <span class="text-purple-400 w-48 shrink-0 truncate" title={key}>{key}</span>
      <span class="break-all">
        {#if changed}
          <span class="line-through text-muted-foreground mr-2">[{bootArr?.join(", ")}]</span>
          <span class="text-orange-400">[{val.join(", ")}]</span>
        {:else}
          <span class="text-green-400">{val.length > 0 ? val.join(", ") : "[]"}</span>
        {/if}
      </span>
    </div>
  {:else}
    <!-- Primitive value -->
    {@const changed = bootVal !== undefined && bootVal !== val}
    <div class="flex py-0.5 {'ml-' + (depth > 0 ? '4' : '0')}">
      <span class="text-purple-400 w-48 shrink-0 truncate" title={key}>{key}</span>
      <span class="break-all">
        {#if changed}
          <span class="line-through text-muted-foreground mr-2">{String(bootVal)}</span>
          <span class="text-orange-400">{String(val)}</span>
        {:else if String(val) === "******"}
          <span class="text-yellow-400">{val}</span>
        {:else if typeof val === "boolean"}
          <span class="{val ? 'text-green-400' : 'text-zinc-500'}">{String(val)}</span>
        {:else if typeof val === "number"}
          <span class="text-cyan-400">{val}</span>
        {:else}
          <span class="text-green-400">{String(val)}</span>
        {/if}
      </span>
    </div>
  {/if}
{/snippet}

<svelte:head>
  <title>Status</title>
</svelte:head>

<div class="min-h-screen px-4 py-2 md:px-8 md:py-3 max-w-6xl mx-auto space-y-3">
  <!-- Title bar with uptime -->
  <div class="flex items-baseline justify-between">
    <h1 class="text-3xl font-bold tracking-tight">nifty-filter
      {#if configJson?.hostname}
        <span class="text-lg font-normal text-muted-foreground ml-2">{configJson.hostname}</span>
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
        {#if configJson == null}
          <Card.Root>
            <Card.Content class="pt-4">
              <p class="text-muted-foreground text-sm">No configuration file found. Create <code class="font-mono text-foreground bg-muted px-1 rounded">/var/nifty-filter/nifty-filter.hcl</code> to get started.</p>
            </Card.Content>
          </Card.Root>
        {:else}
        <div class="flex gap-1 border-b border-border/50 mb-4">
          <button
            class="px-3 py-1.5 text-sm font-medium transition-colors {configSubTab === 'overview'
              ? 'border-b-2 border-primary text-foreground'
              : 'text-muted-foreground hover:text-foreground'}"
            onclick={() => { configSubTab = "overview"; updateHash(); }}
          >Overview</button>
          <button
            class="px-3 py-1.5 text-sm font-medium transition-colors {configSubTab === 'spec'
              ? 'border-b-2 border-primary text-foreground'
              : 'text-muted-foreground hover:text-foreground'}"
            onclick={() => { configSubTab = "spec"; updateHash(); }}
          >Spec</button>
        </div>

        {#if configSubTab === "overview"}
          {@const hostname = configJson?.hostname ?? "nifty-filter"}
          {@const wanIface = configJson?.interfaces?.wan ?? ""}
          {@const trunkIface = configJson?.interfaces?.trunk ?? ""}
          {@const mgmtIface = configJson?.interfaces?.mgmt ?? ""}
          {@const mgmtSubnet = configJson?.interfaces?.mgmt_subnet ?? ""}
          {@const ipv4 = configJson?.wan?.enable_ipv4 === true}
          {@const ipv6 = configJson?.wan?.enable_ipv6 === true}
          {@const vlanSwitch = configJson?.vlan_aware_switch === true}
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
          <!-- Spec sub-tab: hierarchical HCL config view -->
          <p class="text-sm text-muted-foreground mb-4">This page shows the HCL configuration spec for nifty-filter. Edit <code class="font-mono text-foreground bg-muted px-1 rounded">/var/nifty-filter/nifty-filter.hcl</code> and your changes will appear here immediately, but will not be applied until you reboot. Changes since boot are shown in <span class="text-orange-400">orange</span>.</p>
          <Card.Root>
            <Card.Content class="pt-2">
              {#if configJson}
                <div class="font-mono text-sm space-y-0">
                  {#each Object.entries(configJson) as [key, val]}
                    {@render specNode(key, val, bootConfigJson?.[key], 0)}
                  {/each}
                </div>
              {:else}
                <p class="text-muted-foreground text-sm">No configuration loaded.</p>
              {/if}
            </Card.Content>
          </Card.Root>
        {/if}
        {/if}

      {:else if activeTab === "state"}
        <div class="flex gap-1 border-b border-border/50 mb-4">
          {#each stateSubTabs as sub}
            {#if sub.condition()}
              <button
                class="px-3 py-1.5 text-sm font-medium transition-colors {stateSubTab === sub.id
                  ? 'border-b-2 border-primary text-foreground'
                  : 'text-muted-foreground hover:text-foreground'}"
                onclick={() => { stateSubTab = sub.id; updateHash(); }}
              >
                {sub.label}
              </button>
            {/if}
          {/each}
        </div>
        {#if stateSubTab === "interfaces" && data.interfaces.length > 0}
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

        {:else if stateSubTab === "nftables" && data.nft_chains.length > 0}
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

        {:else if stateSubTab === "dnsmasq" && dnsmasqData}
        <div class="space-y-4">
          <!-- Upstream DNS -->
          {#if dnsmasqData.upstream_dns.length > 0}
          <Card.Root>
            <Card.Header class="pb-2">
              <Card.Title>Upstream DNS</Card.Title>
            </Card.Header>
            <Card.Content>
              <div class="flex flex-wrap gap-2">
                {#each dnsmasqData.upstream_dns as server}
                  <span class="font-mono text-sm bg-muted px-2 py-1 rounded">{server}</span>
                {/each}
              </div>
            </Card.Content>
          </Card.Root>
          {/if}

          <!-- Per-interface DHCP config -->
          {#if dnsmasqData.interfaces.length > 0}
          <Card.Root>
            <Card.Header class="pb-2">
              <Card.Title>DHCP Interfaces</Card.Title>
            </Card.Header>
            <Card.Content>
              <div class="overflow-x-auto">
                <table class="w-full text-sm">
                  <thead>
                    <tr class="border-b border-border text-left text-muted-foreground">
                      <th class="py-2 pr-4">Interface</th>
                      <th class="py-2 pr-4">Router</th>
                      <th class="py-2 pr-4">Pool</th>
                      <th class="py-2 pr-4">DNS</th>
                      <th class="py-2 pr-4">Lease</th>
                      <th class="py-2">IPv6</th>
                    </tr>
                  </thead>
                  <tbody class="font-mono">
                    {#each dnsmasqData.interfaces as iface}
                      <tr class="border-b border-border/50">
                        <td class="py-2 pr-4 font-semibold">{iface.name}</td>
                        <td class="py-2 pr-4">{iface.dhcp_router ?? "-"}</td>
                        <td class="py-2 pr-4">{iface.pool_start && iface.pool_end ? `${iface.pool_start} – ${iface.pool_end}` : "-"}</td>
                        <td class="py-2 pr-4">{iface.dhcp_dns ?? "-"}</td>
                        <td class="py-2 pr-4">{iface.lease_time ?? "-"}</td>
                        <td class="py-2">
                          {#if iface.pool_start_v6}
                            <span class="text-green-400" title="{iface.pool_start_v6} – {iface.pool_end_v6}">DHCPv6{iface.ra_enabled ? " + RA" : ""}</span>
                          {:else}
                            -
                          {/if}
                        </td>
                      </tr>
                    {/each}
                  </tbody>
                </table>
              </div>
            </Card.Content>
          </Card.Root>
          {/if}

          <!-- Static hosts -->
          {#if dnsmasqData.static_hosts.length > 0}
          <Card.Root>
            <Card.Header class="pb-2">
              <Card.Title>Static Leases</Card.Title>
            </Card.Header>
            <Card.Content>
              <div class="overflow-x-auto">
                <table class="w-full text-sm">
                  <thead>
                    <tr class="border-b border-border text-left text-muted-foreground">
                      <th class="py-2 pr-4">MAC</th>
                      <th class="py-2 pr-4">IP</th>
                      <th class="py-2">Hostname</th>
                    </tr>
                  </thead>
                  <tbody class="font-mono">
                    {#each dnsmasqData.static_hosts as host}
                      <tr class="border-b border-border/50">
                        <td class="py-2 pr-4">{host.mac}</td>
                        <td class="py-2 pr-4">{host.ip}</td>
                        <td class="py-2">{host.hostname ?? "-"}</td>
                      </tr>
                    {/each}
                  </tbody>
                </table>
              </div>
            </Card.Content>
          </Card.Root>
          {/if}

          <!-- Active leases per VLAN -->
          {#each dnsmasqData.interfaces as iface}
            {@const subnet = iface.dhcp_router ? iface.dhcp_router.split('.').slice(0, 3).join('.') + '.' : null}
            {@const ifaceLeases = subnet ? dnsmasqData.leases.filter(l => l.ip.startsWith(subnet)) : []}
            {#if ifaceLeases.length > 0}
            <Card.Root>
              <Card.Header class="pb-2">
                <Card.Title>Active Leases — {iface.name}</Card.Title>
              </Card.Header>
              <Card.Content>
                <div class="overflow-x-auto">
                  <table class="w-full text-sm">
                    <thead>
                      <tr class="border-b border-border text-left text-muted-foreground">
                        <th class="py-2 pr-4">IP</th>
                        <th class="py-2 pr-4">MAC</th>
                        <th class="py-2 pr-4">Hostname</th>
                        <th class="py-2">Expires</th>
                      </tr>
                    </thead>
                    <tbody class="font-mono">
                      {#each ifaceLeases as lease}
                        <tr class="border-b border-border/50">
                          <td class="py-2 pr-4">{lease.ip}</td>
                          <td class="py-2 pr-4">{lease.mac}</td>
                          <td class="py-2 pr-4">{lease.hostname === "*" ? "-" : lease.hostname}</td>
                          <td class="py-2">{lease.expires === "0" ? "static" : new Date(parseInt(lease.expires) * 1000).toLocaleString()}</td>
                        </tr>
                      {/each}
                    </tbody>
                  </table>
                </div>
              </Card.Content>
            </Card.Root>
            {/if}
          {/each}

          {#if !dnsmasqData.config_found}
          <Card.Root>
            <Card.Content class="py-4">
              <p class="text-muted-foreground text-sm">dnsmasq configuration not found at /run/dnsmasq.conf</p>
            </Card.Content>
          </Card.Root>
          {/if}
        </div>

        {:else if stateSubTab === "qos" && qosData}
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

        {:else if stateSubTab === "switch" && data.switch}
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
        {/if}

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
              <div>
                <h3 class="text-xs text-muted-foreground mb-1">License (MIT)</h3>
                <div class="w-full bg-muted/30 border border-border rounded-md p-3 font-mono text-xs text-muted-foreground whitespace-pre-wrap">{aboutData.license}</div>
              </div>
            </div>
          </Card.Content>
        </Card.Root>
      {/if}
    </div>
  {/if}
</div>
