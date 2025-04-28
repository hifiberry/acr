# rust-debian-package

This project is a Rust application packaged for Debian. It includes both the source code for the application and the necessary files to create a Debian package.

## Project Structure

```
rust-debian-package
├── src
│   ├── main.rs       # Entry point of the Rust application
│   └── lib.rs        # Library code for reusable functions and modules
├── debian
│   ├── changelog     # Changelog for the Debian package
│   ├── control       # Package metadata
│   ├── copyright     # Copyright and licensing information
│   ├── rules         # Makefile for building and installing the package
│   └── compat        # Debhelper compatibility level
├── Cargo.toml        # Rust project configuration
├── Cargo.lock        # Dependency versions for reproducible builds
├── build.rs          # Custom build script
└── README.md         # Project documentation
```

## Building the Project

To build the Rust application, run the following command in the project root:

```
cargo build --release
```

## Creating the Debian Package

To create the Debian package, navigate to the `debian` directory and run:

```
dpkg-buildpackage -us -uc
```

This will generate a `.deb` file in the parent directory.

## Installation

Once the Debian package is created, you can install it using:

```
sudo dpkg -i ../<package-name>.deb
```

Replace `<package-name>` with the actual name of the generated package.

## License

This project is licensed under the MIT License. See the `debian/copyright` file for more details.