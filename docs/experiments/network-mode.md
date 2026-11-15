# Experiment: network mode

Status: idea. Nothing is built. See the [roadmap](../roadmap.md#experiments); planned as
milestone [M-0006](../../sdlc/milestones/M-0006-network.md).

## Goal

Dashboards on the same network find each other. From one, you can see another's shared screens
and tiles after entering its password. The data between them is encrypted.

For example: a home server shares its `ops` screen, and a laptop on the same Wi-Fi lists it
under "Network" and shows it, or puts one of its tiles on a local screen.

## Words

"Published" already means a stability tier in the roadmap. Here a dashboard **shares** screens
and tiles, and a **peer** is another dashboard on the network.

## What it would look like

The sharing side names what it shares. Nothing else is visible from the network.

```yaml
experimental: [network]
network:
  name: home-server              # shown to peers; default: the host name
  share:
    screens: [ops]
    modules: [uptime]            # single tiles, beyond the screens
  password_env: DASHBOARD_SHARE_PASSWORD     # or password_command, as with API tokens
```

The viewing side lists peers in the web view's settings ("Network") and opens a shared screen
read-only. A peer's tile can also be put on a local screen as a module:

```yaml
modules:
  server-uptime:
    builtin: remote
    config: { peer: home-server, tile: uptime }
```

A remote tile is tagged `remote` in both views, as a demo tile is tagged `demo`.

## Design sketch

- **Discovery:** mDNS / DNS-SD (`_osdlc-dashboard._tcp.local`). The record carries the name, the
  version, the port and a fingerprint of the peer's key. No data goes in it. Crate to try:
  `mdns-sd` (pure Rust).
- **A second listener:** the web server binds 127.0.0.1 only today, and that stays as it is. Network
  mode adds a separate listener on the network that serves only the shared tiles. It is read-only:
  no config writes, no settings, no commands.
- **Password and encryption:** a PAKE (SPAKE2 or CPace) turns the password into a session key
  without sending the password. Someone who records the exchange can't guess the password
  offline. That rules out keying an encrypted channel directly from the password. Each dashboard
  also has a long-lived key, pinned the first time a peer connects, so a later impostor shows up.
  Crates to try: `spake2`, then `chacha20poly1305` or `snow` (Noise) for the channel.
- **What is sent:** the same tile messages the renderers get ([contract.md](../contract.md)),
  filtered to the shared tiles, and the shared screens' layout.
- **What is never sent:** the config file or its path, module `config:` blocks, tokens, commands.
  An `open_url` action is sent as it is; it opens on the viewer's machine.
- **Wrong passwords** are rate limited per address.

## Steps

1. Discovery only: `dashboard peers` lists the dashboards on the network.
2. One shared screen, encrypted and read-only, shown in the web view.
3. The `remote` module, and the TUI.
4. A write-up: what worked, what it costs (binary size, idle CPU), and whether it moves into a
   milestone.

## Open questions

- One password per dashboard, or one per shared screen?
- Local network only, or later across networks through a relay?
- Does a viewer get the series history on connect, or only new points?
- Should a peer be able to revoke a viewer without changing the password?

## Done when

Two machines on one network see each other. One shows the other's shared screen after the
password is entered, and a packet capture shows no plaintext. The write-up decides whether
network mode becomes milestone work.
