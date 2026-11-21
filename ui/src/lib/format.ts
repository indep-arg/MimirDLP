// How sizes, speeds and times are written. Numbers only: the words around
// them are translated where they are shown.

const megabytes = (bytes: number) => (bytes / 1_000_000).toFixed(1);

/** "12.3 MB" */
export function size(bytes: number): string {
  return `${megabytes(bytes)} MB`;
}

/** How much of `total` is done, from 0 to 1; 0 when the total isn't known. */
export function fraction(done: number, total: number | null): number {
  if (!total || total <= 0) return 0;
  return Math.min(1, done / total);
}

/** "12.3 / 45.6 MB  (27%)", or just the amount when the total isn't known. */
export function amount(done: number, total: number | null): string {
  if (!total || total <= 0) return size(done);
  const percent = Math.round(fraction(done, total) * 100);
  return `${megabytes(done)} / ${megabytes(total)} MB  (${percent}%)`;
}

/** Megabytes per second, one decimal. */
export function speed(bytesPerSecond: number): string {
  return megabytes(Math.floor(bytesPerSecond));
}

/** "45s", "12m 05s" or "1h 02m". */
export function duration(seconds: number): string {
  const s = Math.max(0, Math.round(seconds));
  const two = (n: number) => String(n).padStart(2, "0");
  if (s < 60) return `${s}s`;
  if (s < 3600) return `${Math.floor(s / 60)}m ${two(s % 60)}s`;
  return `${Math.floor(s / 3600)}h ${two(Math.floor((s % 3600) / 60))}m`;
}

/** A video's length as players write it: "9:57" or "1:02:03". */
export function clock(seconds: number): string {
  const s = Math.max(0, Math.round(seconds));
  const two = (n: number) => String(n).padStart(2, "0");
  const [h, m] = [Math.floor(s / 3600), Math.floor((s % 3600) / 60)];
  return h > 0 ? `${h}:${two(m)}:${two(s % 60)}` : `${m}:${two(s % 60)}`;
}
