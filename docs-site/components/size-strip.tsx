import styles from '@/components/size-strip.module.css';
import sizes from '@/lib/sizes.json';

/**
 * Format a byte count the way a package registry does: decimal units, one decimal place.
 * @param bytes - The count.
 * @returns The count in KB or MB.
 */
export const formatSize = (bytes: number): string => {
  if (bytes >= 1_000_000) {
    return `${(bytes / 1_000_000).toFixed(1)} MB`;
  }
  const kilobytes = bytes / 1000;
  return `${kilobytes >= 100 ? kilobytes.toFixed(0) : kilobytes.toFixed(1)} KB`;
};

/** The representative prebuild: the one `scripts/measure-sizes.mjs` measures from the installed package. */
const nodePlatform = 'linux-x64-gnu';
/** The representative wheel: the one `pip download` fetched on the measuring host. */
const wheelTag = 'cp313-cp313-manylinux_2_28_x86_64';

const list = (entries: Readonly<Record<string, number>>): string =>
  Object.entries(entries)
    .map(([key, bytes]) => `${key} ${formatSize(bytes)}`)
    .join(', ');

/** One cell of the strip: the figure, what it measures, and a hover hint naming every sibling figure. */
export type SizeCell = {
  readonly label: string;
  readonly what: string;
  readonly bytes: number;
  readonly hint: string;
};

/** The three cells, in the order the strip shows them. */
export const sizeCells: readonly SizeCell[] = [
  {
    label: 'Browser',
    what: 'WASM, brotli',
    bytes: sizes.wasm.brotli,
    hint: `WebAssembly engine, brotli-compressed; ${formatSize(sizes.wasm.raw)} raw, ${formatSize(sizes.wasm.gzip)} gzip`,
  },
  {
    label: 'Node',
    what: `${nodePlatform} prebuild`,
    bytes: sizes.native[nodePlatform],
    hint: `native addon per platform, unpacked: ${list(sizes.native)}`,
  },
  {
    label: 'Python',
    what: 'manylinux x86_64 wheel',
    bytes: sizes.wheels[wheelTag],
    hint: `wheel per host: ${list(sizes.wheels)}`,
  },
];

/**
 * The measured download size of each GeoSpec surface, read from `lib/sizes.json`, which
 * `scripts/measure-sizes.mjs` writes and `lib/sizes.test.ts` holds to the shipped bytes.
 * @returns The strip.
 */
export const SizeStrip = (): React.JSX.Element => (
  <dl className={styles.strip}>
    {sizeCells.map(({ label, what, bytes, hint }, index) => (
      <div className={index === 0 ? styles.lead : undefined} key={label} title={hint}>
        <dt>{label}</dt>
        <dd>
          {formatSize(bytes)}
          <span className={styles.what}>{what}</span>
        </dd>
      </div>
    ))}
  </dl>
);
