# ER-C Data Management - Frontend

React SPA for the ER-C Data Management Self-Service portal ("RDM 2026: UI"). Replaces the
legacy server-rendered Flask + Jinja2 + MUI UI incrementally.

**Stack:**

- Vite
- React + TypeScript
- Tailwind CSS v4
- React Router (data mode).

## Develop

```bash
npm install
npm run dev
```

Then open **`http://localhost:5180`**.

The backend is Linux-only and does not run on macOS, so the dev server **proxies `/api` (and
`/login`, `/logout`, `/browse`, `/static`) to a running backend** — see `vite.config.ts`. It
defaults to `http://localhost:5001`, which the SSH tunnel below makes point at the VM, so you
normally **don't** need to set anything. (To override, put `VITE_API_PROXY=http://host:port`
in a git-ignored `.env.local`. **Never point it at production.**)

## Run the app against the VM backend — full step-by-step

The React app runs on the Mac; the backend runs in a **UTM** virtual machine on the same Mac.
The Mac reaches the VM's backend through an **SSH tunnel**, and the dev server proxies `/api`
to it. Follow the steps exactly.

### This machine's actual setup (real values, not placeholders)

| Thing                          | Value                                                                                                                                       |
|--------------------------------|---------------------------------------------------------------------------------------------------------------------------------------------|
| VM app                         | UTM, running Debian 12                                                                                                                      |
| VM login                       | username `debian`, password `debian`                                                                                                        |
| VM IP (UTM **Shared Network**) | `192.168.64.2` — if SSH ever can't connect, get the current one by running `hostname -I` **in the VM** and using its `192.168.64.x` address |
| Backend address inside the VM  | `127.0.0.1:5001`                                                                                                                            |
| App URL on the Mac             | `http://localhost:5180`                                                                                                                     |

### First-time-only setup (do these once, ever)

1. In **UTM**: select the VM → Settings → **Network → Mode = Shared Network**.
2. In the **VM** terminal, install + start the SSH server:
   ```bash
   sudo apt install -y openssh-server && sudo systemctl enable --now ssh
   ```

### Every time you want to test — 4 steps

**Step 1 — start the backend (in the VM terminal).** Build from source; never pull.

```bash
cd ~/Desktop/data-management-system/deploy

docker compose up
```

Confirm it booted (still in the VM):

```bash
curl -I http://127.0.0.1:5001/login/      # expect: HTTP/1.1 200 or 302, no Python traceback
```

**Step 2 — open the SSH tunnel (in a Mac terminal).** Leave this window open the whole time.

```bash
ssh -N -L 5001:127.0.0.1:5001 debian@192.168.64.2
```

- Password: `debian`. First time, type `yes` at the fingerprint prompt.
- It will sit there with **no output and no prompt** — that is correct (`-N` just holds the
  tunnel open). **Do not close this window.** Closing it kills the connection.

**Step 3 — confirm the Mac can reach the VM (a second Mac terminal).**

```bash
curl -i http://localhost:5001/api/project/      # expect: HTTP/1.1 401 UNAUTHORIZED
```

That `401` is the **success** signal — you reached the backend and it's demanding a login.

**Step 4 — run the app (Mac terminal).**

```bash
cd frontend && npm run dev
```

Open **`http://localhost:5180`** and log in with your real IFF/ER-C Unix username +
password. Correct creds → you land on the Projects page with your name in the header.

### When it breaks — check here first

| What you see                                                       | What it means / fix                                                                                                                        |
|--------------------------------------------------------------------|--------------------------------------------------------------------------------------------------------------------------------------------|
| `ssh: connect to host ... port 22: Connection refused`             | SSH server not running in the VM → do the first-time setup. Or the IP changed → run `hostname -I` in the VM, use the `192.168.64.x` value. |
| `hostname -I` shows `10.0.2.15`                                    | UTM is in NAT mode → switch to **Shared Network**, restart the VM.                                                                         |
| `curl http://localhost:5001/...` on the Mac → `Connection refused` | The tunnel window (Step 2) is closed. Reopen it and keep it open.                                                                          |
| `/api` calls in the app fail after it was working                  | Same — the tunnel window got closed. Reopen Step 2.                                                                                        |

# Build

```bash
npm run build    # type-check (tsc -b) + production bundle into dist/
npm run preview  # serve the built bundle locally
```

## Theming

Design tokens are CSS custom properties in [`src/styles/theme.css`](src/styles/theme.css)
(`:root` = light, `.dark` = dark), exposed to Tailwind via `@theme inline`. Components use
semantic utilities only (`bg-surface`, `text-fg`, `border-default`, `bg-primary`, …) and
never hardcode hex — toggling the `dark` class on `<html>` switches the whole theme.
