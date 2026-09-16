# Troubleshooting

Start with **Settings → Help → Troubleshooter** in the desktop app: it checks the firewall, network profile,
virtual microphone, audio devices and permissions, and offers a fix for each problem it finds. It also spots three
things people otherwise chase for a while:

- a **VPN** that carries your whole connection, which normally blocks the local network too — turn on "Allow LAN
  traffic" in the VPN's settings, or disconnect it while you stream;
- a **network that blocks devices from talking to each other**, which is how guest and hotel Wi-Fi are usually set
  up: SoundPush knows where your phone is and still cannot reach it, so use the phone's hotspot or a USB cable;
- **audio-enhancement software** on Windows (Nahimic, Sonic Studio and the like) that sits in the sound path and can
  make recordings silent.

If support asks you to change something that is not in the ordinary settings, it will be in
[Advanced settings](advanced.md).

--8<-- "docs/user-guide.md:troubleshooting"
