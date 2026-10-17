import { isIPv4 } from "node:net";
import { networkInterfaces, type NetworkInterfaceInfo } from "node:os";

function privateAddress(address: string): boolean {
  if (!isIPv4(address)) {
    return false;
  }
  const [first, second] = address.split(".").map(Number);
  return (
    first === 10 ||
    (first === 172 && second !== undefined && second >= 16 && second <= 31) ||
    (first === 192 && second === 168)
  );
}

/** Advertise an assigned LAN address, never a loopback or container-only default. */
export function localAddress(
  interfaces: Record<string, NetworkInterfaceInfo[] | undefined> = networkInterfaces(),
  override?: string,
): string {
  const assigned = Object.values(interfaces)
    .flatMap((entries) => entries ?? [])
    .filter((entry) => !entry.internal && entry.family === "IPv4" && privateAddress(entry.address));
  if (override !== undefined) {
    if (!assigned.some((entry) => entry.address === override)) {
      throw new Error("LOCAL_LAN_IP must be a private IPv4 address assigned to this computer");
    }
    return override;
  }
  const addresses = [
    ...new Set(
      Object.entries(interfaces)
        .filter(([name]) => !/^(docker|veth|br-|virbr|tun|tap)/.test(name))
        .flatMap(([, entries]) => entries ?? [])
        .filter(
          (entry) => !entry.internal && entry.family === "IPv4" && privateAddress(entry.address),
        )
        .map((entry) => entry.address),
    ),
  ];
  if (addresses.length !== 1 || addresses[0] === undefined) {
    throw new Error(
      "Cannot select one LAN address; set LOCAL_LAN_IP to this computer's Wi-Fi or Ethernet IPv4 address",
    );
  }
  return addresses[0];
}

const hostname =
  /^(?=.{1,253}$)(?:[a-z0-9](?:[a-z0-9-]{0,61}[a-z0-9])?\.)+[a-z](?:[a-z0-9-]{0,61}[a-z0-9])?$/;

/** Validate the public DNS name or IPv4 address that remote players use to reach this host. */
export function publicHost(value: string): string {
  const host = value.trim().toLowerCase();
  if (
    isIPv4(host)
      ? host.split(".").every((part) => String(Number(part)) === part)
      : hostname.test(host)
  ) {
    return host;
  }
  throw new Error(
    "LOCAL_PUBLIC_HOST must be a DNS name such as play.example.com or an IPv4 address",
  );
}
