<script>
  import { onMount } from "svelte";
  import { Button } from "$lib/components/ui/button/index.js";
  import * as Card from "$lib/components/ui/card/index.js";
  import { Input } from "$lib/components/ui/input/index.js";
  import { FieldGroup, Field, FieldLabel } from "$lib/components/ui/field/index.js";

  const {
    id,
    whoami = "/api/whoami",
    loginAction = "/api/login",
    logoutAction = "/api/logout",
  } = $props();

  let username = $state("");
  let password = $state("");

  let csrfToken = $state("");
  let isLoggedIn = $state(false);

  // NEW: tracks the very first whoami load
  let initializing = $state(true);

  // existing: tracks button actions
  let loading = $state(false);
  let errorMsg = $state("");

  function applyWhoami(json) {
    const session = json?.data?.session;
    csrfToken = typeof session?.csrf_token === "string" ? session.csrf_token : "";
    username = typeof session?.username === "string" ? session.username : "";
    isLoggedIn = !!session?.is_logged_in;
  }

  async function fetchWhoami() {
    try {
      const res = await fetch(whoami, { credentials: "include" });
      const json = await res.json().catch(() => null);
      applyWhoami(json);
      return { res, json };
    } finally {
      // only matters for the first render, but harmless on later calls
      initializing = false;
    }
  }

  onMount(fetchWhoami);

  async function ensureCsrf() {
    if (csrfToken) return true;
    await fetchWhoami();
    return !!csrfToken;
  }

  async function postJson(url, bodyObj) {
    return fetch(url, {
      method: "POST",
      credentials: "include",
      headers: {
        "Content-Type": "application/json",
        "X-CSRF-Token": csrfToken,
      },
      body: bodyObj ? JSON.stringify(bodyObj) : "{}",
    });
  }

  async function submitLogin(event) {
    event.preventDefault();
    loading = true;
    errorMsg = "";

    if (!(await ensureCsrf())) {
      loading = false;
      errorMsg = "Could not retrieve CSRF token. Please refresh and try again.";
      return;
    }

    let res = await postJson(loginAction, { username, password });
    let json = await res.json().catch(() => null);

    if (
      res.status === 401 &&
      (json?.error?.code === "csrf_invalid" || json?.error?.code === "csrf_missing")
    ) {
      await fetchWhoami();
      res = await postJson(loginAction, { username, password });
      json = await res.json().catch(() => null);
    }

    if (!res.ok || json?.error) {
      errorMsg = json?.error?.message ?? json?.error ?? `Login failed (HTTP ${res.status})`;
      loading = false;
      return;
    }

    await fetchWhoami();
    password = "";
    loading = false;
  }

  async function submitLogout() {
    loading = true;
    errorMsg = "";

    if (!(await ensureCsrf())) {
      loading = false;
      errorMsg = "Could not retrieve CSRF token. Please refresh and try again.";
      return;
    }

    let res = await postJson(logoutAction, null);
    let json = await res.json().catch(() => null);

    if (
      res.status === 401 &&
      (json?.error?.code === "csrf_invalid" || json?.error?.code === "csrf_missing")
    ) {
      await fetchWhoami();
      res = await postJson(logoutAction, null);
      json = await res.json().catch(() => null);
    }

    if (!res.ok || json?.error) {
      errorMsg = json?.error?.message ?? json?.error ?? `Logout failed (HTTP ${res.status})`;
      loading = false;
      return;
    }

    await fetchWhoami();
    loading = false;
  }
</script>

<Card.Root class="mx-auto w-full max-w-sm">
  <Card.Header>
    {#if initializing}
      <Card.Title class="text-2xl">Loading…</Card.Title>
      <Card.Description>Checking your session…</Card.Description>
    {:else if isLoggedIn}
      <Card.Title class="text-2xl">You’re signed in</Card.Title>
      <Card.Description>
        Signed in as <span class="font-mono">{username || "(unknown)"}</span>
      </Card.Description>
    {:else}
      <Card.Title class="text-2xl">Sign in</Card.Title>
      <Card.Description>Enter your account credentials:</Card.Description>
    {/if}
  </Card.Header>

  <Card.Content>
    {#if errorMsg}
      <p class="mb-3 text-sm text-red-600">{errorMsg}</p>
    {/if}

    {#if initializing}
      <!-- Optional: keep body empty to avoid any “login” flash -->
      <div class="h-10"></div>
    {:else if isLoggedIn}
      <Button class="w-full" onclick={submitLogout} disabled={loading}>
        {loading ? "Signing out…" : "Logout"}
      </Button>
    {:else}
      <form onsubmit={submitLogin}>
        <FieldGroup>
          <Field>
            <FieldLabel for={"username-" + id}>Username</FieldLabel>
            <Input
              id={"username-" + id}
              type="username"
              placeholder="username"
              autocomplete="username"
              required
              bind:value={username}
            />
          </Field>

          <Field>
            <FieldLabel for={"password-" + id}>Password</FieldLabel>
            <Input
              id={"password-" + id}
              type="password"
              autocomplete="current-password"
              required
              bind:value={password}
            />
          </Field>

          <Field>
            <Button type="submit" class="w-full" disabled={loading}>
              {loading ? "Logging in…" : "Login"}
            </Button>
          </Field>
        </FieldGroup>
      </form>
    {/if}
  </Card.Content>
</Card.Root>
