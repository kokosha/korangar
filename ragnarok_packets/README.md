# Ragnarok Packets

A crate that exposes types for Ragnarok Online server-client communication.

## Examples

### Packet capture

An example that uses the `PacketHandler` to deserialize packets captured with `libpcap` and print them to `stdout`.
Since `pcap` requires privileges to monitor your network traffic, the compiled example needs them as well.


The easiest way is to not use `cargo run` and instead build with
```bash
cargo build --example pcap --features unicode
```

##### Hint: Make sure you have `libpcap` installed on your system, otherwise the build will fail.
##### Hint: You can add the `unicode` feature for some slightly nicer output if your system supports it.


And then run the resulting binary in `target/debug/examples/pcap` as root or admin. E.g.
```bash
sudo target/debug/examples/pcap
```

### Pcap pre-requisite
The `pcap` crate has some needed pre-requisite software installation.

#### Windows
1 - Install [NpCap](https://npcap.com/#download) using the installer.

2 - During installation, remember to select the WinPcap API-compatible mode.

3 - Download the [Npcap SDK](https://npcap.com/#download) files.

4 - Extract the Lib directory from Npcap SDK files

5 - Depending on the system Windows 86 or 64 or using ARM.

##### Hint: Make sure you have the correct `wpcap.lib` and `Packet.lib` inside the lib or else the software won't link and you will take some time to found out why it isn't linking.

#### Linux
1 - Install `libpcap`.

The package inside the Linux can be one of these variants `libpcap`, `libpcap-dev` or `libpcap-devel`.