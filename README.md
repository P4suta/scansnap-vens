# `scansnap-vens`

Share a ScanSnap iX1300 with eSCL clients over Wi-Fi.

## Installation

```sh
git clone https://github.com/P4suta/scansnap-vens.git
cd scansnap-vens
cargo install --locked --path crates/vens-cli
cargo install --locked --path crates/vens-daemon
```

## Usage

```sh
vensd --address 192.0.2.7 --consume /path/to/consume
```

## Documentation

[Read the documentation](docs/README.md).

## License

Licensed under the [Apache License, Version 2.0](LICENSE-APACHE) or the [Massachusetts Institute of Technology (MIT) license](LICENSE-MIT), at your option.

Contributions use both licenses unless you state other terms.
