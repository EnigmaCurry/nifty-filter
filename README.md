# nifty-filter

> 💥 warning: alpha pre-release version

nifty-filter is a template and configuration tool for
[netfilter](https://www.netfilter.org/) (nftables) and is useful for
creating a Linux based Internet protocol (IP) router. It is a program
that generates the `nftables.nft` config file, using its own internal
template. The configuration is done entirely by environment variables
(or `.env` file) and the output is type checked and validated.

The jinja-like template is powered by
[djc/askama](https://github.com/djc/askama), which implements compile
time type checking of input values. Therefore, if you wish to
customize the template, you will have to compile your own nifty-filter
binary. However, the default template is designed to cover most of the
use cases for a typical home LAN router, so if that suits your needs
then you can simply download the precompiled binary from the releases
page.

## Install

[Download the latest release for your platform.](https://github.com/EnigmaCurry/nifty-filter/releases)

Or install via cargo ([crates.io/crates/nifty-filter](https://crates.io/crates/nifty-filter)):

```
cargo install nifty-filter
```

## Examples

There are several included [examples](examples):

 * [home_router.sh](examples/home_router.sh) - This example is a
   self-contained bash script where all config is defined inside the
   script as environment variables.
   
 * [home_router.env](examples/home_router.env) - This example is a
   "dot env" file containging all the config variables. You can pass
   this to `nifty-router --ignore-env --env-file [FILE]`.
   
 
You can mix the two styles together as long you don't specify
`--ignore-env`, in which case only the file passed to `--env-file`
will be used.
   
## Config styles

You can supply your configuration in two ways: set environment
variables and/or provide a `.env` file.

Example with a `.env` file that ignores all other environment
variables:

```bash
nifty-filter --env-file .env --ignore-env
```

Example with a `.env` file and mixing it with some outside environment
variables:

```bash
INTERFACE_LAN=eth0 \
INTERFACE_WAN=eth1 \
nifty-filter --env-file .env
```

Example with only environment variables (but this is an incomplete
config):

```bash
INTERFACE_LAN=eth0 \
INTERFACE_WAN=eth1 \
nifty-filter
```

Run with extra validation which passes the output to `nft -c -f -` for
sanity checking:

```
#...
nifty-filter --validate
```
