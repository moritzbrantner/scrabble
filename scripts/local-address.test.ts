import { expect, test } from "bun:test";
import { type NetworkInterfaceInfo } from "node:os";

import { localAddress, publicHost } from "./local-address";

function ipv4(address: string, internal = false): NetworkInterfaceInfo {
  return {
    address,
    internal,
    family: "IPv4",
    netmask: "255.255.255.0",
    mac: "00:00:00:00:00:00",
    cidr: `${address}/24`,
  };
}

test("LAN detection ignores loopback and container bridges", () => {
  expect(
    localAddress({
      lo: [ipv4("127.0.0.1", true)],
      docker0: [ipv4("172.17.0.1")],
      "br-container": [ipv4("172.18.0.1")],
      enp6s0: [ipv4("192.168.178.22")],
    }),
  ).toBe("192.168.178.22");
});

test("ambiguous networks need an explicit assigned private address", () => {
  const interfaces = { ethernet: [ipv4("192.168.1.10")], wifi: [ipv4("10.0.0.10")] };
  expect(() => localAddress(interfaces)).toThrow("Cannot select one LAN address");
  expect(localAddress(interfaces, "10.0.0.10")).toBe("10.0.0.10");
  for (const address of ["127.0.0.1", "192.168.1.11", "1.1.1.1", "invalid"]) {
    expect(() => localAddress(interfaces, address)).toThrow("assigned to this computer");
  }
  expect(() => localAddress({ lo: [ipv4("127.0.0.1", true)] })).toThrow(
    "Cannot select one LAN address",
  );
});

test("public hosts are DNS names or canonical IPv4 addresses", () => {
  expect(publicHost("Play.Example.com")).toBe("play.example.com");
  expect(publicHost("203.0.113.7")).toBe("203.0.113.7");
  for (const host of [
    "",
    "localhost",
    "https://play.example.com",
    "play.example.com:5173",
    "a..b",
    "010.0.0.1",
  ]) {
    expect(() => publicHost(host)).toThrow("LOCAL_PUBLIC_HOST");
  }
});
