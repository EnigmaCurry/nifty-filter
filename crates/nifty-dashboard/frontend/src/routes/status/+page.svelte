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
  }

  interface ConfigSection {
    name: string;
    entries: ConfigEntry[];
  }

  type Tab = "config" | "interfaces" | "nftables" | "switch" | "about";

  interface AboutData {
    version: string;
    repository: string;
    license: string;
  }

  type ConfigSubTab = "overview" | "environment";

  let data = $state<StatusData | null>(null);
  let configData = $state<ConfigSection[]>([]);
  let aboutData = $state<AboutData | null>(null);
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
    const validTabs: Tab[] = ["config", "interfaces", "nftables", "switch", "about"];
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
    { id: "switch", label: "Switch", condition: () => data?.switch != null },
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
    fetchStatus();
    const interval = setInterval(fetchStatus, 15000);

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
    <h1 class="text-3xl font-bold tracking-tight">nifty-filter</h1>
    {#if data?.uptime}
      <span class="text-sm text-muted-foreground">
        up <span class="font-mono text-foreground">{formatUptime(data.uptime.uptime_seconds)}</span>
      </span>
    {/if}
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
          {@const wanIface = cfgVal("INTERFACE_WAN")}
          {@const trunkIface = cfgVal("INTERFACE_TRUNK")}
          {@const mgmtIface = cfgVal("INTERFACE_MGMT")}
          {@const mgmtSubnet = cfgVal("SUBNET_MGMT")}
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
          <Card.Root>
            <Card.Content class="pt-2">
              <table class="w-full text-sm" style="table-layout:fixed">
                <colgroup>
                  <col style="width: 18rem;" />
                  <col />
                </colgroup>
                <tbody class="font-mono">
                  {#each configData as section, sIdx}
                    <tr class="bg-muted/30">
                      <td colspan="2" class="py-2 px-2 font-sans font-semibold text-sm {sIdx > 0 ? 'pt-4' : ''}">{section.name}</td>
                    </tr>
                    {#each section.entries as entry}
                      <tr class="{entry.is_commented_out ? 'opacity-30' : ''}" title={entry.comment ?? ""}>
                        <td class="py-0.5 pr-2 text-purple-400 whitespace-nowrap overflow-hidden text-ellipsis">{entry.key}</td>
                        <td class="py-0.5 {entry.value === '******' ? 'text-yellow-400' : 'text-green-400'} break-all">{entry.value || '""'}</td>
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
                        <span class={stateColor(iface.state)}>{iface.state}</span>
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
                <textarea readonly class="w-full h-96 bg-muted/30 border border-border rounded-md p-3 font-mono text-xs text-muted-foreground resize-none focus:outline-none">{aboutData.license}</textarea>
              </div>
            </div>
          </Card.Content>
        </Card.Root>
      {/if}
    </div>
  {/if}
</div>
