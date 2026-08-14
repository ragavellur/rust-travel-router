# travel-net VPN — Setup Guide (plain English)

This guide is written for people who don't work with networks. If you can use a
phone app, you can do this. Take it one step at a time.

## What does the VPN do?

Picture this:

- One travel-net box stays at **home**, plugged in and always on.
- The other travel-net box **travels with you**.
- You connect your laptop to the travel box's Wi-Fi (just like a hotel Wi-Fi).
- Everything you do goes through a **secret tunnel** back to your home box and
  out to the internet from your home.

Result:

- Websites and your employer see your **home address**, not the hotel's.
- You can reach your **home devices** (NAS, cameras, printer) from anywhere.
- Your data is encrypted even on public Wi-Fi.

> Use a VPN **inside your travel box's Wi-Fi**? You don't need to. The travel
> box does the VPN for every device connected to it. No software on your
> laptop or phone.

---

## Before you start

You need **two boxes with the same plan**:

1. A travel-net box that will live at **home** (the "home box").
2. A travel-net box you will **carry** (the "travel box").

One of these can be replaced by an **existing computer at home** (a Raspberry
Pi, an old laptop, a NAS) — that is explained below for both options.

**Pick one VPN type:**

| | Tailscale | WireGuard |
|---|---|---|
| Easy? | **Very easy.** | A few more steps. |
| Works anywhere? | Yes, even without opening ports on your router. | Yes, but your home router needs **one port forwarded** (UDP 51820). |
| Free? | Yes (Personal plan up to 100 devices). | Yes, fully free. |
| Who controls it? | Tailscale's free service coordinates the tunnel. | You. No third party. |

Choose **Tailscale** if you want the simplest path. Choose **WireGuard** if you
prefer no third party or you already use WireGuard at home.

---

# PART 1 — TAILSCALE

## Step 1. Create a Tailscale account (5 minutes, on any device)

1. Go to https://tailscale.com and click **Get started / Sign up**.
2. Sign in with your Google, Microsoft, or Apple account.
3. You'll land in the **Admin Console** (login.tailscale.com). That's your
   control panel. Keep this tab open.

## Step 2. Make the "exit key" on your home box

### Option A — your home box is a travel-net box

1. Connect to the home box's web console (`http://192.168.4.1`).
2. Go to the **VPN** page.
3. Set **Role = Home box**, **Backend = Tailscale**.
4. In the Admin Console, go to **Settings → Keys → Generate auth key**.
   Copy it, paste it into the **Auth key** box on the VPN page, and press
   **Save & Apply**.
5. After a minute, refresh the VPN page. It should show your home box
   connected with a name like `home-box`.

### Option B — your home box is an existing computer (Raspberry Pi / Linux)

1. Open a terminal **on that computer** and install Tailscale:

   ```bash
   curl -fsSL https://tailscale.com/install.sh | sh
   ```

2. Turn it into an "exit node" (so travel traffic can leave through it) and
   tell it about your home network. Run this one command (replace
   `192.168.1.0/24` with your home network — usually the router's own IP with
   a `/24` at the end):

   ```bash
   sudo tailscale up --advertise-exit-node --advertise-routes=192.168.1.0/24
   ```

   A link will appear — open it and log in with the same account.
3. In the Admin Console, find your home machine and click the **three dots
   (⋮) → Approve** for **Use as exit node** and **Subnet routes**. (Settings →
   **Edit route settings** or the machine's **Routes** tab.)

> ⚠️ The home machine must be **always on**. If it sleeps, the tunnel sleeps.

## Step 3. Set up the travel box

1. Connect to the **travel box's** web console (`http://192.168.4.1`).
2. Go to the **VPN** page.
3. Set **Role = Travel box**, **Backend = Tailscale**.
4. **Auth key**: generate another key in the Admin Console (same place as
   Step 2) and paste it here. (You can reuse one key, but one per box is tidy.)
5. **Home node (exit node)**: the name of your home machine in Tailscale,
   e.g. `home-box` or `home-pi`.
6. Leave **Keep hotel Wi-Fi reachable** on. Press **Save & Apply**.

## Step 4. Verify (2 minutes)

1. Connect a laptop/phone to the **travel box's Wi-Fi**.
2. In a browser, search **"what is my IP"**.
3. It should show your **home** city/IP, even though you're elsewhere.

> Not working? Refresh the Admin Console — approve the travel box (any new
> machine needs one-time approval), then press **Save & Apply** again on the
> travel box's VPN page.

---

# PART 2 — WIREGUARD

WireGuard is two sides of a key:

- The **home side** is the "server" — it owns the address you dial into.
- The **travel side** is the "client" — it dials in.

Every travel-net box has a **secret key** it keeps private and a **public key**
it can safely show. The two sides trade public keys and a tunnel address, then
connect.

> **Quickest path:** if both boxes are travel-net, follow **2A** (the boxes
> guide you). If your home side is an existing computer, follow **2B**.
> If you already have a WireGuard config file from a VPN provider (ProtonVPN,
> Mullvad, …), skip to **2C**.

## Step 2A. Both boxes are travel-net

1. **Travel box** → **VPN** page → Role **Travel box**, Backend **WireGuard**.
2. Click **Generate my key**. The page shows your **public key** and a small
   "request block". Copy it.
3. **Home box** → **VPN** page → Role **Home box**, Backend **WireGuard**.
   Set **Listen port** (leave 51820). Click **Generate server key**.
4. In the **Peers** section click **Add peer**, paste the travel box's
   **public key**, and give the travel box a tunnel address like `10.0.0.2`
   (or use the suggested one). Save.
5. The home box now shows a **ready-to-paste client config**. Copy it.
6. Back on the **travel box**, paste that into the **Import .conf** box
   (or the fields) and press **Save & Apply**.
7. Both boxes should show **Connected** with a handshake.

### If your home router is in between (port forwarding)

Your home box answers on **UDP port 51820**. Tell your home router to forward
that port to the home box's IP address:

1. Log into your home router (usually `192.168.1.1`).
2. Find **Port Forwarding** (sometimes under Advanced / NAT / Applications).
3. Add: **Protocol UDP, external port 51820 → internal IP <home box IP>,
   internal port 51820**.
4. Save/apply.

If your home internet does not give you a public address (ask your ISP if
unsure), WireGuard will not work from outside — **use Tailscale instead** in
that case.

## Step 2B. Home side is an existing Linux computer / Raspberry Pi

Do these steps **on that home machine** (a terminal), once. It is the same
thing the home box does for you automatically.

1. Install WireGuard:

   ```bash
   sudo apt update && sudo apt install -y wireguard wireguard-tools
   ```

2. Create the secret keys:

   ```bash
   cd /tmp
   umask 077
   wg genkey | tee server_private.key | wg pubkey > server_public.key
   ```

   Leave this terminal open — you'll need the keys.

3. Create the server config. Replace the parts in `<>`:

   ```bash
   sudo tee /etc/wireguard/wg0.conf > /dev/null <<'EOF'
   [Interface]
   PrivateKey = <paste contents of server_private.key>
   Address = 10.0.0.1/24
   ListenPort = 51820

   [Peer]
   # My travel box
   PublicKey = <TRAVEL_BOX_PUBLIC_KEY - see step 5 below>
   AllowedIPs = 10.0.0.2/32, 192.168.4.0/24
   EOF
   ```

   > `10.0.0.1/24` is the home side's address inside the tunnel.
   > `192.168.4.0/24` is the travel box's own network, so it can answer back.

4. Start the server:

   ```bash
   sudo sysctl -w net.ipv4.ip_forward=1
   echo "net.ipv4.ip_forward=1" | sudo tee -a /etc/sysctl.conf
   sudo iptables -t nat -A POSTROUTING -o eth0 -j MASQUERADE
   sudo systemctl enable --now wg-quick@wg0
   ```

   (Replace `eth0` with your machine's internet interface — check with
   `ip route show default`.)

5. On the **travel box** VPN page: Role **Travel box**, Backend **WireGuard**,
   click **Generate my key**, and copy the **public key**. Paste it above into
   the home machine's `wg0.conf` (the `PublicKey = ...` line), then restart:

   ```bash
   sudo systemctl restart wg-quick@wg0
   ```

6. Allow the port through the firewall (if you have one):

   ```bash
   sudo ufw allow 51820/udp
   ```

7. Set up **port forwarding** on the home router exactly as described at the
   end of Step 2A (UDP 51820 → the home machine's IP).

8. Now configure the travel box:
   - **Peer public key**: the contents of `server_public.key`
     (`cat /tmp/server_public.key`)
   - **Endpoint**: your home address + `:51820` (e.g. `myhome.ddns.net:51820`
     or your public IP `:51820`)
   - **Tunnel address**: `10.0.0.2/24`
   - Press **Save & Apply**. It should show **Connected** with a handshake.

## Step 2C. Using a VPN provider's WireGuard file

Providers such as ProtonVPN and Mullvad give you a `*.conf` file.

1. On the travel box → VPN page → Backend **WireGuard**.
2. Open the `.conf` file in any text editor, copy the whole content.
3. Paste it into **Import .conf** and press **Import**.
4. Review the filled fields, then **Save & Apply**.

---

## Verifying and troubleshooting

**Check the connection**
- VPN page shows a green **Connected** and a recent handshake.
- Search "what is my IP" while connected to the travel box — it should be your
  home address.

**Tailscale: box won't connect**
- Admin Console → approve the new machine (⋮ → Approve).
- On the travel box press **Save & Apply** again.

**WireGuard: "no handshake"**
- Is UDP 51820 forwarded on the home router? (Step 2A)
- Keys not swapped? Public key of the server goes on the client and vice
  versa.
- Is the home box/server running and always-on?

**Everything broke after the tunnel dropped**
- That's the **kill switch** doing its job — travel devices lose internet
  instead of leaking your real location. When the tunnel reconnects, internet
  returns automatically.

**I changed roles / backend and nothing works**
- Press **Save & Apply** after every change, then reload the page.
- Turning the VPN **None** restores normal router behaviour.

---

## Safety notes

- The **home box must stay on** while you're away — that's the whole point.
- The **secret keys** shown on the VPN pages never leave your boxes; don't
  paste them anywhere except where a step tells you.
- Use this feature to respect your employer's rules and local laws. Don't use
  it to bypass your company's security policies.
