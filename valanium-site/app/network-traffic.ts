// Read only default-route interface counters: adding tunnels or loopback would
// count the same transfer twice. Interface names and counters stay server-side.
export type NetworkSample = Record<string, { rx: number; tx: number }>;
export function networkSnapshot(routes: string, devices: string): NetworkSample | null {
  const interfaces = new Set(routes.trim().split('\n').slice(1).flatMap(line => {
    const fields = line.trim().split(/\s+/);
    return fields[1] === '00000000' && fields[7] === '00000000' && (parseInt(fields[3], 16) & 1) && fields[0] !== 'lo' ? [fields[0]] : [];
  }));
  if (!interfaces.size) return null;
  const sample: NetworkSample = {};
  for (const line of devices.split('\n')) {
    const colon = line.lastIndexOf(':');
    if (colon < 0) continue;
    const name = line.slice(0, colon).trim();
    if (!interfaces.has(name)) continue;
    const fields = line.slice(colon + 1).trim().split(/\s+/).map(Number);
    if (fields.length < 16 || ![fields[0], fields[8]].every(n => Number.isSafeInteger(n) && n >= 0)) return null;
    sample[name] = { rx: fields[0], tx: fields[8] };
  }
  return Object.keys(sample).length === interfaces.size ? sample : null;
}
export function networkRates(before: NetworkSample | null, after: NetworkSample | null, seconds: number) {
  const missing = { rxMbps: null, txMbps: null };
  if (!before || !after || !Number.isFinite(seconds) || seconds <= 0 || seconds > 10
      || Object.keys(before).sort().join('|') !== Object.keys(after).sort().join('|')) return missing;
  let rx = 0, tx = 0;
  for (const name of Object.keys(before)) {
    const received = after[name].rx - before[name].rx, sent = after[name].tx - before[name].tx;
    if (received < 0 || sent < 0) return missing;
    rx += received; tx += sent;
  }
  const rate = (bytes: number) => {
    const value = bytes * 8 / seconds / 1_000_000;
    return Number.isFinite(value) && value <= 1_000_000 ? Math.round(value * 100) / 100 : null;
  };
  return { rxMbps: rate(rx), txMbps: rate(tx) };
}
