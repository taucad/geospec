import { readMatcherReferenceRows } from '@/components/matcher-reference-registry.ts';

/**
 * Every capability in the engine's registry, generated at build time from `registry.rs`, with a link
 * to its reference section. "Reads faces" is the registry's own classification: a STEP subject whose
 * faces carry no surface refuses those capabilities. The site builds from its own directory.
 * @returns The registry table.
 */
export const MatcherReference = (): React.JSX.Element => (
  <div className="overflow-x-auto">
    <table data-matcher-registry="">
      <thead>
        <tr>
          <th>Capability</th>
          <th>Kind</th>
          <th>Reads faces</th>
        </tr>
      </thead>
      <tbody>
        {readMatcherReferenceRows(process.cwd()).map(({ name, kind, readsFaces, href }) => (
          <tr data-capability={name} key={name}>
            <td>
              {href === undefined ? (
                <code>{name}</code>
              ) : (
                <a href={href}>
                  <code>{name}</code>
                </a>
              )}
            </td>
            <td>{kind}</td>
            <td>{readsFaces}</td>
          </tr>
        ))}
      </tbody>
    </table>
  </div>
);
