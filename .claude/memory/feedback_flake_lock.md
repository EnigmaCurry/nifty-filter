---
name: Update nixos-vm-template flake lock after nifty-filter changes
description: Always update nixos-vm-template flake lock after pushing any nifty-filter changes
type: feedback
---

After pushing nifty-filter changes, always update the nixos-vm-template flake lock before committing there: `cd ../nixos-vm-template && nix flake lock --update-input nifty-filter`

**Why:** The flake lock pins a specific commit hash, not a branch. Any nifty-filter change (Nix modules or Rust crate code) won't be picked up by nixos-vm-template builds until the lock is updated. This caused a build failure when Nix saw conflicting definitions from old vs new code.

**How to apply:** Every time you push to nifty-filter and plan to commit/push nixos-vm-template, run the flake lock update first.
