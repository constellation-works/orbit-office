/** The license line. Shown only while a private endpoint is configured. */
export const MUSE_SPARK_OPEN_ATTRIBUTION = "Built with Muse Spark Open";

/** Settings tells the shell the endpoint was saved, without the shell importing a screen. */
export const PRIVATE_ROUTE_EVENT = "office-private-route";

/** Empty when nothing is configured, so the footer can hide. */
export function footerText(configured: boolean): string {
  return configured ? MUSE_SPARK_OPEN_ATTRIBUTION : "";
}

export function applyFooter(footer: HTMLElement, configured: boolean): void {
  const text = footerText(configured);
  if (text === "") {
    footer.replaceChildren();
    return;
  }
  const line = document.createElement("p");
  line.textContent = text;
  footer.replaceChildren(line);
}
