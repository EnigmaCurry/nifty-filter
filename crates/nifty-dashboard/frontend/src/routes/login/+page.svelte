<script lang="ts">
  import { onMount } from "svelte";
  import LoginOidc from "$lib/components/login_oidc.svelte";
  import LoginForm from "$lib/components/login_form.svelte";
  import * as Card from "$lib/components/ui/card/index.js";

  type AuthMethod = "oidc" | "username_password";

  let authMethod = $state<AuthMethod | null>(null);
  let loading = $state(true);
  let errorMsg = $state("");

  function normalizeAuthMethod(v: unknown): AuthMethod | null {
    if (typeof v !== "string") return null;
    const s = v.trim().toLowerCase().replaceAll("-", "_");
    if (s === "oidc") return "oidc";
    if (s === "username_password") return "username_password";
    return null;
  }

  async function loadConfig() {
    loading = true;
    errorMsg = "";

    try {
      const res = await fetch("/api/config", { credentials: "include" });

      const ct = res.headers.get("content-type") ?? "";
      const body = ct.includes("application/json")
        ? await res.json().catch(() => null)
        : await res.text().catch(() => "");

      if (!res.ok) {
        errorMsg =
          typeof body === "string"
            ? `Config request failed (HTTP ${res.status}). Body: ${body.slice(0, 200)}`
            : `Config request failed (HTTP ${res.status}).`;
        return;
      }

      if (typeof body === "string" || body == null) {
        errorMsg = `Expected JSON from /api/config but got ${typeof body}.`;
        return;
      }

      const raw = body?.data?.config?.auth_method;
      const m = normalizeAuthMethod(raw);

      if (!m) {
        errorMsg = `Invalid auth_method from server: ${JSON.stringify(raw)}`;
        return;
      }

      authMethod = m;
    } catch (e) {
      errorMsg = `Could not load server configuration. ${String(e)}`;
    } finally {
      loading = false;
    }
  }

  onMount(loadConfig);
</script>

<div class="flex h-screen w-full items-center justify-center px-4">
  {#if loading || errorMsg}
    <Card.Root class="mx-auto w-full max-w-sm">
      <Card.Header>
        <Card.Title class="text-2xl">
          {#if loading}Loading…{:else}Error{/if}
        </Card.Title>
        <Card.Description>
          {#if loading}
            Checking server configuration…
          {:else}
            Something went wrong loading the app config.
          {/if}
        </Card.Description>
      </Card.Header>

      <Card.Content>
        {#if errorMsg}
          <p class="text-sm text-red-600">{errorMsg}</p>
        {:else}
          <!-- spacer to match the button/form area height -->
          <div class="h-10"></div>
        {/if}
      </Card.Content>
    </Card.Root>
  {:else if authMethod === "oidc"}
    <LoginOidc id="login" loginAction="/api/login" logoutAction="/api/logout" />
  {:else}
    <LoginForm id="login" loginAction="/api/login" logoutAction="/api/logout" />
  {/if}
</div>
