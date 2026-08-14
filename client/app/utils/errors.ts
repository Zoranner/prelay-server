export interface RelayError {
  code: string;
  message: string;
}

const knownCodes = new Set([
  "identity_already_registered",
  "invalid_credential",
  "not_found",
  "validation_failed",
  "internal",
]);

export function toRelayError(error: unknown): RelayError {
  if (typeof error === "object" && error !== null) {
    const candidate = error as Partial<RelayError>;
    if (typeof candidate.code === "string" && typeof candidate.message === "string") {
      return { code: candidate.code, message: candidate.message };
    }
  }

  const message = error instanceof Error ? error.message : String(error);
  const code = [...knownCodes].find((knownCode) => message.includes(knownCode));
  return { code: code ?? "internal", message };
}
