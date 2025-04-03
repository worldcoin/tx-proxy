# tx-proxy: World Chain Transaction Relay Service

`tx-proxy` is a proxy server that sits between Alchemy's transaction relays, our block builders, and the sequencing op-geth nodes on World Chain. The service multiplexes incoming `eth_sendRawTransaction` requests to two highly available backends.

Its primary purpose is to ensure transactions are validated by builders before being relayed to Op-Geth sequencers. 

![](diagram.png)

## License

Unless otherwise specified, all code in this repository is dual-licensed under
either:

- MIT License ([LICENSE-MIT](LICENSE-MIT))
- Apache License, Version 2.0, with LLVM Exceptions
  ([LICENSE-APACHE](LICENSE-APACHE))

at your option. This means you may select the license you prefer to use.

Any contribution intentionally submitted for inclusion in the work by you, as
defined in the Apache-2.0 license, shall be dual licensed as above, without any
additional terms or conditions.

