/** How long-lived Node hosts keep admitted subjects between runs: counts, memory and their defaults. @module */

import { memoryUsage } from 'node:process';

/**
 * How many admitted subjects a long-lived engine keeps between shards, files and runs, with the facts it has
 * measured on them, and the memory above which it keeps none.
 *
 * @public
 */
export type GeoSpecSubjectRetention = {
  /** Subjects kept after each release, least recently loaded released first. */
  readonly subjects: number;
  /** Process resident set size, in bytes, above which a release keeps no subject. */
  readonly memoryBytes: number;
};

/**
 * The retention a long-lived engine defaults to: 64 subjects, and none above 2 GiB resident. A retained STEP subject
 * whose facts were measured holds its parsed document, about two to five times its file size (0.2 MiB for a file
 * under 100 KB, 2.5 MiB near 0.5 MB, 12 MiB near 2.6 MB, 50 MiB near 19 MB, measured on Linux), and one admitted from
 * the facts store holds a copy of its bytes until a claim opens it. A worker's loader also keeps every retained
 * model's bytes within the engine's 128 MiB binary limit, which bounds retention of large models before the count.
 *
 * @public
 */
export const defaultGeoSpecSubjectRetention: GeoSpecSubjectRetention = {
  subjects: 64,
  memoryBytes: 2 * 1024 * 1024 * 1024,
};

/**
 * The subjects a retaining engine admits at most: the kept count plus room for a shard's new admissions, never below
 * the engine's entry profile of 32.
 *
 * @param retention - The retention.
 * @returns The engine's `maxRetainedSubjects`.
 * @internal
 */
export const retainedSubjectCapacity = ({
  subjects = defaultGeoSpecSubjectRetention.subjects,
}: Partial<GeoSpecSubjectRetention>): number =>
  Number.isSafeInteger(subjects) && subjects > 0 ? Math.max(32, subjects + 8) : 32;

/**
 * The count of subjects to keep at a release: none while the process's resident set exceeds the retention's memory.
 * An absent field takes its default; a memory that is not a number keeps none.
 *
 * @param retention - The retention.
 * @param residentBytes - Reads the resident set size; the process's own outside tests.
 * @returns Reads the count to keep now.
 * @internal
 */
export const retainedSubjectCount =
  (
    {
      subjects = defaultGeoSpecSubjectRetention.subjects,
      memoryBytes = defaultGeoSpecSubjectRetention.memoryBytes,
    }: Partial<GeoSpecSubjectRetention>,
    residentBytes: () => number = () => memoryUsage.rss(),
  ) =>
  (): number =>
    residentBytes() <= memoryBytes ? subjects : 0;
