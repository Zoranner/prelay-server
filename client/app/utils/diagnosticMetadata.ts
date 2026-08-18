const maxMetadataLength = 12_000;

export function formatDiagnosticMetadata(metadata: string | null): string | null {
  if (!metadata?.trim()) return null;

  let formatted = metadata;
  try {
    formatted = JSON.stringify(JSON.parse(metadata), null, 2);
  } catch {
    // Keep invalid metadata visible without allowing it to interrupt diagnostics.
  }

  return formatted.length > maxMetadataLength
    ? `${formatted.slice(0, maxMetadataLength)}...（已截断）`
    : formatted;
}
