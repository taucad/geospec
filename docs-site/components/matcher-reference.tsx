import type { CapabilityKind, MatcherReferenceVariant } from '@/components/matcher-reference-registry.ts';
import {
  cheatSheetCells,
  cheatSheetColumns,
  pythonNameOf,
  readCapabilitySheetRows,
  readMatcherReferenceRows,
} from '@/components/matcher-reference-registry.ts';

/**
 * A capability's name in the language on screen: the registry name for TypeScript readers, the
 * snake_case method for Python readers. Both are in the HTML; the stylesheet shows one.
 * @param props - The capability.
 * @param props.name - The registry name.
 * @param props.kind - Its kind; a query keeps its name in Python.
 * @returns The two spellings.
 */
const CapabilityName = ({
  name,
  kind,
}: {
  readonly name: string;
  readonly kind: CapabilityKind;
}): React.JSX.Element => (
  <>
    <code data-face="ts">{name}</code>
    <code data-face="py">{pythonNameOf(name, kind)}</code>
  </>
);

/**
 * A cell whose text may carry backticked code, rendered as `<code>`. The label is the cell's column,
 * which a phone shows beside the value once the table is stacked (`rehype-table-labels.ts`).
 * @param props - The cell.
 * @param props.value - The text.
 * @param props.label - The column's name.
 * @returns The cell.
 */
const Cell = ({ value, label }: { readonly value: string; readonly label: string | undefined }): React.JSX.Element => (
  <td data-label={label} role="cell">
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
    <table data-capability-sheet="" data-stack="" role="table">
      <thead role="rowgroup">
        <tr role="row">
          {cheatSheetColumns.map((column) => (
            <th key={column} role="columnheader">
              {column}
            </th>
          ))}
        </tr>
      </thead>
      <tbody role="rowgroup">
        {readCapabilitySheetRows(process.cwd()).map((row) => (
          <tr key={row.name} role="row">
            <td role="cell">
              <a href={row.href}>
                <CapabilityName name={row.name} kind={row.kind} />
              </a>
            </td>
            {cheatSheetCells(row).map((cell, index) => (
              <Cell key={cheatSheetColumns[index + 1]} label={cheatSheetColumns[index + 1]} value={cell} />
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
 * sheet adds each section's admitted subjects, failure code and refusal codes. The site builds from
 * its own directory.
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
      <table data-matcher-registry="" data-stack="" role="table">
        <thead role="rowgroup">
          <tr role="row">
            <th role="columnheader">Capability</th>
            <th role="columnheader">Kind</th>
            <th role="columnheader">Reads faces</th>
          </tr>
        </thead>
        <tbody role="rowgroup">
          {readMatcherReferenceRows(process.cwd()).map(({ name, kind, readsFaces, href }) => (
            <tr data-capability={name} key={name} role="row">
              <td role="cell">
                {href === undefined ? (
                  <CapabilityName name={name} kind={kind} />
                ) : (
                  <a href={href}>
                    <CapabilityName name={name} kind={kind} />
                  </a>
                )}
              </td>
              <td data-label="Kind" role="cell">
                {kind}
              </td>
              <td data-label="Reads faces" role="cell">
                {readsFaces}
              </td>
            </tr>
          ))}
        </tbody>
      </table>
    </div>
  );
