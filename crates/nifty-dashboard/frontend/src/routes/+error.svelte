<script>
  import { Button } from "$lib/components/ui/button";

  export let error;
  export let status;
  const code = status ?? 404;
</script>

<svelte:head>
  <title>{status} – Not found</title>
</svelte:head>

<main class="min-h-screen flex flex-col items-center justify-center gap-4 px-4 text-center">
  <div class="space-y-2">
    <h1 class="text-4xl font-semibold tracking-tight sm:text-5xl">
      {code}
    </h1>
    <p class="text-base text-muted-foreground max-w-md">
      {error?.message ?? "Sorry, we couldn't find that page."}
    </p>
  </div>

  <Button asChild>
    <a href="/">Go back home</a>
  </Button>
</main>
