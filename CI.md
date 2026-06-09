# CI Setup Guide

This project uses [Woodpecker CI](https://woodpecker-ci.org/) to
automatically build the nifty-filter VM image and publish it to
S3-compatible storage.

## Prerequisites

- A Woodpecker CI agent VM — follow the
  [nixos-vm-template CI setup guide](https://github.com/EnigmaCurry/nixos-vm-template/blob/master/CI.md)
  to deploy Forgejo, Woodpecker, and the agent VM. Both
  nixos-vm-template and nifty-filter share the same agent.
- An S3-compatible storage bucket (e.g., DigitalOcean Spaces, AWS S3,
  MinIO)

## 1. Push to Forgejo

Create a repository on your Forgejo instance and push nifty-filter to
it:

```bash
git remote add forgejo git@forgejo.example.com:youruser/nifty-filter.git
git push forgejo dev
```

## 2. Set Up S3 Storage

Create an S3-compatible bucket for storing the nifty-filter image. This
can be the same provider as nixos-vm-template but should be a separate
bucket.

For DigitalOcean Spaces:

1. Create a Space (e.g., `nifty-filter`)
2. Generate an API key with read/write access to the Space (or reuse
   the key from nixos-vm-template if using the same account)
3. Note the endpoint (e.g., `nyc3.digitaloceanspaces.com`)

## 3. Configure CI Secrets

The pipeline needs S3 credentials to upload images. Configure them
using the provided Justfile recipe.

Find your `WOODPECKER_SERVER` and `WOODPECKER_TOKEN` values at
`https://woodpecker.example.com/user/cli-and-api` (replace with your
Woodpecker server URL).

```bash
export WOODPECKER_SERVER=https://woodpecker.example.com
export WOODPECKER_TOKEN=your-api-token
export CI_REPO=youruser/nifty-filter
export S3_BUCKET=nifty-filter
export S3_PUBLIC_URL=https://nifty-filter.nyc3.cdn.digitaloceanspaces.com
export S3_PROVIDER=DigitalOcean    # or AWS, Minio
export S3_ENDPOINT=nyc3.digitaloceanspaces.com
export S3_REGION=nyc3
export S3_ACCESS_KEY_ID=your-access-key
just ci-secrets
```

The recipe will prompt for the S3 secret access key interactively.

## 4. Pipeline

The pipeline is defined in `.woodpecker.yml`. On each push to `dev`
(or manual trigger) it:

1. **Builds** the VM image using `nix build .#pve-image --impure`
2. **Exports** it with a release filename
   (e.g., `nifty-filter-20260609-abc1234.qcow2`)
3. **Uploads** to S3, replacing any previous image
4. **Updates** `manifest.json` in the bucket root with the download
   URL and sha256 checksum

The published image is built without SSH keys baked in. Keys are added
on first boot via the `just pve-install` wizard or manually via the
console.
