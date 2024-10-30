# nifty-filter

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
