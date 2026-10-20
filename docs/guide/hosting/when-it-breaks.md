# When it breaks

The problems people actually run into, in the order they usually happen.
Each one: what you see, why, and what to do.

::: tip The first thing to try, always
If the install stopped, **run the same line again**.
It remembers what already worked and carries on from where it stopped.
It never asks the same question twice.
:::

## During the install

### "The name … does not lead anywhere yet"

**Why:** your address (for example `app.example.com`) is not connected to your machine yet.

1. Sign in to the company you bought the address from.
2. Open its **DNS** page (sometimes called **Zone** or **DNS records**).
3. Add a record with exactly what the message printed:

   | Type | Name | Value |
   |---|---|---|
   | `A` | the word the message shows (`@` for the bare address) | the numbers the message shows |

4. Save.
5. Wait five minutes.
6. Run the same line again.

::: details The "Name" box is the usual mistake
For `example.com`, type `@` (some sites want the box left empty).
For `app.example.com`, type only `app`, not the full address.
:::

### "The name … does not lead to this machine"

**Why:** the address points somewhere else: an old machine, or a parking page from the company that sold it.

1. Open the same **DNS** page.
2. Delete every other `A` record with that name.
3. Keep only the one with the value the message printed.
4. Wait five minutes, then run the same line again.

If you changed it only a few minutes ago, answer **yes** when the installer offers to wait.
It checks every 20 seconds for up to 10 minutes.

### "The address https://… is not answering yet"

**Why:** almost always one of two things.

- You pointed the address at the machine only minutes ago. **Wait ten minutes**, then run the same line again.
- Your provider has its own firewall in front of the machine, and it is closed.

To open the provider's firewall:

1. Open your provider's control panel.
2. Find the machine's **Firewall** or **Security** settings.
3. Allow incoming **TCP 80** and **TCP 443**.
4. Run the same line again.

### "Something on this machine is already answering on port 80 / 443"

**Why:** your provider installed a web server on the machine for you. It takes the place OpenTraderWorld needs.

Paste this, then run the same line again:

```bash
systemctl disable --now apache2 nginx caddy 2>/dev/null; true
```

### "This machine has … MB of memory" or "Only … MB of disk space is free"

**Why:** the machine is too small. OpenTraderWorld needs about **2 GB of memory** and **8 GB of free disk**.

1. At your provider, resize the machine to a bigger plan.
2. Run the same line again.

### "This installer only knows Ubuntu and Debian"

**Why:** the machine was created with another system.

1. At your provider, **reinstall** (or **rebuild**) the machine with **Ubuntu 24.04** or **Debian 13**.
2. Run the same line again.

### "This needs the machine's administrator rights"

**Why:** you are signed in with an account that is not allowed to install software.

Run the line again with `sudo` in the middle, exactly as the message shows:

```bash
curl -fsSL https://get.opentraderworld.com/configure_install.sh | sudo bash -s -- --domain app.example.com
```

## After the install

### The page does not open any more

1. Sign in to your machine.
2. Type:

   ```bash
   otw status
   ```

3. If it says **Nothing is running** or **is not answering**, type:

   ```bash
   otw restart
   ```

4. Wait one minute, then reload the page.

### I can no longer sign in to the machine itself

**Why:** the installer tightened how the machine lets people in.

- If you chose **the key**: sign in from the same computer you used on install day. Passwords are refused on purpose.
- If you chose **a password**: sign in with the account name shown on your card, **not** `root`. Direct `root` sign-in is refused on purpose.

Lost that computer or that password? Use the **console** (sometimes called **VNC**, **rescue** or **web terminal**) in your provider's control panel. It works even when the normal way in is closed.

### I forgot the OpenTraderWorld password

1. Sign in to your machine.
2. Type (replace `admin` with your login name if you changed it):

   ```bash
   docker exec -it opentraderworld-core-1 /app/otw-core reset-password admin
   ```

3. Sign in to the app with the password it prints. The app asks you to choose a new one.

To see your address and login name again, type `otw card`.

### The browser shows "Not secure" or refuses the page

- Type the address with `https://` in front.
- If it started a few minutes after install, wait ten minutes: the padlock is still being issued.
- On a home install (not a rented server), see [Troubleshooting](/guide/troubleshooting).

### The machine is full

**Why:** nightly backups and downloaded price history take space over time.

1. Type `otw status` to see how much space is left.
2. At your provider, give the machine a bigger disk.
3. Type `otw restart`.

## Still stuck?

1. Sign in to your machine.
2. Type:

   ```bash
   otw report
   ```

3. It writes one file and prints where it is. The file holds no password.
4. Open an issue on [GitHub](https://github.com/G-OTW/OpenTraderWorld/issues), say what you were doing, and attach that file.
