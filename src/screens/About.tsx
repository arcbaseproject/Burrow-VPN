export default function About() {
  return (
    <div className="flex flex-col gap-8 px-4 pt-4 pb-6 leading-relaxed">
      <div>
        <h2 className="t-figure text-[34px]">Burrow</h2>
        <p className="text-sm text-fog">Version 0.1.0</p>
        <p className="mt-3 max-w-[44ch]">
          Burrow connects you to WireGuard servers you already have. It runs no servers of its own, so your public IP changes only while
          you're connected to one.
        </p>
      </div>

      <section>
        <h2 className="text-[17px] font-semibold">What Burrow sends</h2>
        <p className="mt-1 max-w-[44ch]">
          Only your tunnel traffic. With IP checks on, Burrow also asks ipinfo.io for your address, through the tunnel. It asks while you're
          exposed only if you allow it. Burrow has no telemetry, logs or analytics.
        </p>
      </section>

      <section>
        <h2 className="text-[17px] font-semibold">What a VPN can't hide</h2>
        <p className="mt-1 max-w-[44ch]">
          Your VPN provider sees your real IP and every site you reach. Websites can still recognise your browser, and anything you sign into
          knows it's you. For anonymity, use Tor Browser.
        </p>
      </section>

      <section>
        <h2 className="text-[17px] font-semibold">How the tunnel works</h2>
        <p className="mt-1 max-w-[44ch]">
          On Linux, Burrow drives the WireGuard module built into your kernel. On Windows, it uses the WireGuard for Windows service.
        </p>
      </section>

      <p className="text-sm text-fog">
        WireGuard is a registered trademark of Jason A. Donenfeld. Burrow isn't affiliated with the WireGuard project. MIT licence. Set in
        Archivo by Omnibus-Type, under the SIL Open Font License.
      </p>
    </div>
  );
}
