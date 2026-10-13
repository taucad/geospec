/**
 * The ecosystem link at the foot of the docs sidebar. `order-first` lifts it above the GitHub and
 * theme row, which Fumadocs renders before the footer slot.
 * @returns The attribution link.
 */
export const TauAttributionFooter = (): React.JSX.Element => (
  <a className="order-first mb-3 text-fd-muted-foreground text-xs hover:text-fd-foreground" href="https://tau.new">
    Part of the Tau ecosystem
  </a>
);
