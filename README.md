# sirun 🚨
[![docs.rs](https://docs.rs/sirun/badge.svg)](https://docs.rs/sirun/latest/)

`sirun` (pronounced like "siren") is a tool for taking basic perfomance
measurements of a process covering its entire lifetime. It gets memory and
timing information from the kernel and also allows
[Statsd](https://github.com/statsd/statsd#usage) messages to be sent to
`udp://localhost:8125` (in the future this will be configurable), and those will
be included in the outputted metrics.

It's intended that this tool be used for shorter-running benchmarks, and not for
long-lived processes that don't die without external interaction. You could
certainly use it for long-lived processes, but that's not where it shines.

## Installation

### Via Cargo

`cargo install sirun`

### From releases

[Release bundles](https://github.com/DataDog/sirun/releases) are provided for
each supported plaform. Extract the binary somewhere and use it.

### From source

Make sure you have [`rustup`](https://rustup.rs/) installed, and use that to
ensure you have the latest stable Rust toolchain enabled.

#### From a local repo clone

`cargo install .`

#### Without cloning locally

With SSH

`cargo install --git ssh://git@github.com:22/DataDog/sirun.git --branch main`

or with HTTPS

`cargo install --git https://github.com/DataDog/sirun.git --branch main`

## Usage

See the [documentation](https://docs.rs/sirun/latest/).

## License

Licensed under either of

 * Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE) or http://www.apache.org/licenses/LICENSE-2.0)
 * MIT license ([LICENSE-MIT](LICENSE-MIT) or http://opensource.org/licenses/MIT)

at your option.

### Contribution

Unless you explicitly state otherwise, any contribution intentionally submitted
for inclusion in the work by you, as defined in the Apache-2.0 license, shall be
dual licensed as above, without any additional terms or conditions.
