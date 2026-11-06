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
 * The retention a long-lived engine defaults to. The engine's entry profile retains at most 32 subjects, so 24 leaves
 * room for a shard's or a file's new admissions beside the kept ones; above 2 GiB resident a process keeps none.
 *
 * @public
 */
export const defaultGeoSpecSubjectRetention: GeoSpecSubjectRetention = {
  subjects: 24,
  memoryBytes: 2 * 1024 * 1024 * 1024,
};

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
