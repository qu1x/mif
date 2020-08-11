# mif

Memory Initialization File

[![Build Status][]](https://travis-ci.org/qu1x/mif)
[![Downloads][]](https://crates.io/crates/mif)
[![Rust][]](https://www.rust-lang.org)
[![Version][]](https://crates.io/crates/mif)
[![Documentation][]](https://docs.rs/mif)
[![MIT][]](https://opensource.org/licenses/MIT)
[![Apache-2.0][]](https://opensource.org/licenses/Apache-2.0)

[Build Status]: https://travis-ci.org/qu1x/mif.svg
[Downloads]: https://img.shields.io/crates/d/mif.svg
[Rust]: https://img.shields.io/badge/rust-stable-brightgreen.svg
[Version]: https://img.shields.io/crates/v/mif.svg
[Documentation]: https://docs.rs/mif/badge.svg
[MIT]: https://img.shields.io/badge/License-MIT-blue.svg
[Apache-2.0]: https://img.shields.io/badge/License-Apache--2.0-blue.svg

## Command-line Utility

Provides two subcommands, `dump` and `join`.

```
mif 0.1.0
Rouven Spreckels <rs@qu1x.dev>
Memory Initialization File

USAGE:
    mif <SUBCOMMAND>

OPTIONS:
    -h, --help       Prints help information
    -V, --version    Prints version information

SUBCOMMANDS:
    dump    Dumps binary as MIF
    join    Joins binaries' memory areas to MIFs
    help    Prints this message or the help of the given subcommand(s)
```

### Dump Subcommand

```
mif-dump
Dumps binary as MIF

USAGE:
    mif dump [input]

ARGS:
    <input>    Input file or standard input (-) [default: -]

OPTIONS:
    -w, --width <bits>       Word width in bits from 8 to 128 [default: 16]
    -f, --first <lsb|msb>    LSB/MSB first (little/big-endian) [default: lsb]
    -h, --help               Prints help information
    -V, --version            Prints version information
```

### Join Subcommand

```
mif-join
Joins binaries' memory areas to MIFs

USAGE:
    mif join [OPTIONS] [toml]

ARGS:
    <toml>    TOML file or standard input (-) [default: -]

OPTIONS:
    -i, --bins <path>    Input directory [default: .]
    -o, --mifs <path>    Output directory [default: .]
    -n, --no-comments    No comments in MIFs
    -h, --help           Prints help information
    -V, --version        Prints version information
```

#### Join Example

Assuming two ROM dumps, `a.rom` and `b.rom`, whose program and data areas
are concatenated as in:

  * `cat a.program.rom a.data.rom > a.rom`
  * `cat b.program.rom b.data.rom > b.rom`

Following TOML file defines how to join both program areas to one MIF and
both data areas to another MIF, assuming 24-bit program words of depth 1267
and 1747 and 16-bit data words of depth 1024 each. Additionally, every area
is dumped to its own separate MIF for verification. Then, between program
and data area is supposed to be an unused area of `0xffffff` words, which
should be skipped. Listing them in the `skips` instruction will verify that
this area only contains these words.

```toml
[["a.rom"]]
first = "lsb" # Least-significant byte first. Default, can be omitted.
width = 24
depth = 1267
joins = ["a.prog.mif", "ab.prog.mif"]
[["a.rom"]]
first = "lsb" # Least-significant byte first. Default, can be omitted.
width = 24
depth = 781
skips = [0xffffff] # Empty [] for skipping without verification.
[["a.rom"]]
first = "msb"
width = 16 # Default, can be omitted.
depth = 1024
joins = ["a.data.mif", "ab.data.mif"]

[["b.rom"]]
width = 24
depth = 1747
joins = ["b.prog.mif", "ab.prog.mif"]
[["b.rom"]]
width = 24
depth = 301
skips = [0xffffff]
[["b.rom"]]
depth = 1024
joins = ["b.data.mif", "ab.data.mif"]
```

## License

Dual-licensed under `MIT OR Apache-2.0`.

## Contribution

Unless you explicitly state otherwise, any contribution intentionally submitted
for inclusion in the works by you shall be licensed as above, without any
additional terms or conditions.
