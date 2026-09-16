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

## "A device keeps trying to connect but doesn't recognise this computer"

Devices remember each other by a key they exchange when you pair them, and each one refuses anything else — which is
what keeps a stranger on your network from taking your microphone. So a device that was paired with SoundPush on a
computer that has since been reinstalled, or whose SoundPush data was cleared, no longer recognises it: it keeps
trying to connect, is refused every time, and neither device can do anything about it on its own.

SoundPush tells you when that happens, names the address it is coming from, and records it in the security log
(**Settings → Privacy & security → Security log**). The fix is to pair the two devices again: forget the old entry on
the device that keeps trying, then pair from either side as usual.

Nothing is wrong with your network in this case, and the streams you already have are unaffected.

If support asks you to change something that is not in the ordinary settings, it will be in
[Advanced settings](advanced.md).

--8<-- "docs/user-guide.md:troubleshooting"
