import type { MatcherReferenceVariant } from '@/components/matcher-reference-registry.ts';
import {
  cheatSheetCells,
  cheatSheetColumns,
  readCapabilitySheetRows,
  readMatcherReferenceRows,
} from '@/components/matcher-reference-registry.ts';

/** A cell whose text may carry backticked code, rendered as `<code>`. */
const Cell = ({ value }: { readonly value: string }): React.JSX.Element => (
  <td>
    {value
      .split(/(`[^`]+`)/u)
      .filter((part) => part !== '')
      .map((part, index) =>
        part.startsWith('`') ? <code key={index}>{part.slice(1, -1)}</code> : <span key={index}>{part}</span>,
      )}
  </td>
);

const CheatSheet = (): React.JSX.Element => (
  <div className="overflow-x-auto">
    <table data-capability-sheet="">
      <thead>
        <tr>
          {cheatSheetColumns.map((column) => (
            <th key={column}>{column}</th>
          ))}
        </tr>
      </thead>
      <tbody>
        {readCapabilitySheetRows(process.cwd()).map((row) => (
          <tr key={row.name}>
            <td>
              <a href={row.href}>
                <code>{row.name}</code>
              </a>
            </td>
            {cheatSheetCells(row).map((cell, index) => (
              <Cell key={cheatSheetColumns[index + 1]} value={cell} />
            ))}
          </tr>
        ))}
      </tbody>
    </table>
  </div>
);

/**
 * Every capability in the engine's registry, generated at build time from `registry.rs`, with a link
 * to its reference section. The registry table shows "Reads faces", the registry's own
 * classification: a STEP subject whose faces carry no surface refuses those capabilities. The cheat
 * sheet adds each section's wire kind, admitted subjects, failure code and refusal codes. The site
 * builds from its own directory.
 * @param props - `variant` selects the table; the registry table is the default.
 * @returns The table.
 */
export const MatcherReference = ({
  variant = 'registry',
}: {
  readonly variant?: MatcherReferenceVariant;
}): React.JSX.Element =>
  variant === 'cheat-sheet' ? (
    <CheatSheet />
  ) : (
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
