import { EmailMessage } from "cloudflare:email";
import { createMimeMessage } from "mimetext";

const PUBLIC_RECIPIENT = "hello@pliegors.dev";
const MAX_SUBJECT_LENGTH = 160;
const MAX_REFERENCES = 100;

export async function processEmail(
  message: ForwardableEmailMessage,
  env: Env,
): Promise<void> {
  if (normalizeMailbox(message.to) !== PUBLIC_RECIPIENT) {
    message.setReject("Unknown PliegoRS recipient");
    log("rejected", { reason: "recipient" });
    return;
  }

  const forwardTo = env.FORWARD_TO.trim();
  if (!isMailbox(forwardTo)) {
    throw new Error("FORWARD_TO must be a verified destination mailbox");
  }

  const forwardedHeaders = new Headers({ "X-Processed-By": "PliegoRS-Email" });
  await message.forward(forwardTo, forwardedHeaders);
  log("forwarded", { bytes: message.rawSize });

  const skipReason = autoReplySkipReason(message);
  if (skipReason) {
    log("reply-skipped", { reason: skipReason });
    return;
  }

  try {
    await message.reply(buildReply(message));
    log("replied", {});
  } catch (error) {
    // Forwarding has already succeeded. DMARC or platform reply rejection must
    // not discard the message that a human needs to receive.
    log("reply-failed", { reason: errorName(error) });
  }
}

export function autoReplySkipReason(
  message: Pick<ForwardableEmailMessage, "from" | "to" | "headers">,
): string | null {
  const sender = normalizeMailbox(message.from);
  if (!isMailbox(sender)) return "invalid-sender";
  if (sender.endsWith("@pliegors.dev")) return "same-domain";

  const localPart = sender.slice(0, sender.indexOf("@"));
  if (/^(?:mailer-daemon|postmaster|no-?reply|do-?not-?reply|bounce)/i.test(localPart)) {
    return "automated-sender";
  }

  const autoSubmitted = header(message.headers, "auto-submitted").toLowerCase();
  if (autoSubmitted && autoSubmitted !== "no") return "auto-submitted";

  const precedence = header(message.headers, "precedence").toLowerCase();
  if (["bulk", "junk", "list"].includes(precedence)) return "precedence";
  if (header(message.headers, "list-id")) return "mailing-list";

  const suppress = header(message.headers, "x-auto-response-suppress").toLowerCase();
  if (suppress && suppress !== "none") return "response-suppressed";

  const references = header(message.headers, "references")
    .split(/\s+/)
    .filter(Boolean);
  if (references.length > MAX_REFERENCES) return "reference-limit";

  return null;
}

function buildReply(message: ForwardableEmailMessage): EmailMessage {
  const subject = safeHeader(header(message.headers, "subject"), MAX_SUBJECT_LENGTH);
  const messageId = safeMessageId(header(message.headers, "message-id"));
  const reply = createMimeMessage();

  if (messageId) {
    reply.setHeader("In-Reply-To", messageId);
    const references = header(message.headers, "references");
    reply.setHeader("References", safeReferences(references, messageId));
  }
  reply.setHeader("Auto-Submitted", "auto-replied");
  reply.setHeader("X-Auto-Response-Suppress", "All");
  reply.setSender(PUBLIC_RECIPIENT);
  reply.setRecipient(message.from.trim());
  reply.setSubject(replySubject(subject));
  reply.addMessage({
    contentType: "text/plain",
    data: [
      "Thank you for writing to PliegoRS. Your message reached us and a human will review it.",
      "",
      "Gracias por escribir a PliegoRS. Recibimos tu mensaje y una persona lo revisará.",
      "",
      "https://pliegors.dev",
    ].join("\n"),
  });
  reply.addMessage({
    contentType: "text/html",
    data: "<p>Thank you for writing to <strong>PliegoRS</strong>. Your message reached us and a human will review it.</p><p>Gracias por escribir a <strong>PliegoRS</strong>. Recibimos tu mensaje y una persona lo revisará.</p><p><a href=\"https://pliegors.dev\">pliegors.dev</a></p>",
  });

  return new EmailMessage(PUBLIC_RECIPIENT, message.from.trim(), reply.asRaw());
}

function replySubject(subject: string): string {
  if (!subject) return "Re: PliegoRS";
  return /^re:/i.test(subject) ? subject : `Re: ${subject}`;
}

function safeMessageId(value: string): string {
  const candidate = safeHeader(value, 998);
  return /^<[^<>\s@]+@[^<>\s@]+>$/.test(candidate) ? candidate : "";
}

function safeReferences(value: string, messageId: string): string {
  const references = safeHeader(value, 8_000)
    .split(/\s+/)
    .filter((item) => /^<[^<>\s@]+@[^<>\s@]+>$/.test(item))
    .slice(-(MAX_REFERENCES - 1));
  if (!references.includes(messageId)) references.push(messageId);
  return references.join(" ");
}

function safeHeader(value: string, maxLength: number): string {
  return value.replace(/[\r\n\0]+/g, " ").replace(/\s+/g, " ").trim().slice(0, maxLength);
}

function header(headers: Headers, name: string): string {
  return headers.get(name) ?? "";
}

function normalizeMailbox(value: string): string {
  return value.trim().toLowerCase();
}

function isMailbox(value: string): boolean {
  if (value.length === 0 || value.length > 254) return false;
  const separator = value.indexOf("@");
  if (separator <= 0 || separator !== value.lastIndexOf("@")) return false;

  const local = value.slice(0, separator);
  const domain = value.slice(separator + 1);
  if (local.length > 64 || domain.length === 0 || domain.length > 253) return false;
  for (const character of local) {
    const code = character.charCodeAt(0);
    if (code <= 32 || code >= 127 || character === "<" || character === ">") return false;
  }

  const labels = domain.split(".");
  if (labels.length < 2) return false;
  return labels.every((label) => {
    if (label.length === 0 || label.length > 63 || label.startsWith("-") || label.endsWith("-")) {
      return false;
    }
    for (const character of label) {
      const code = character.charCodeAt(0);
      const alpha = (code >= 65 && code <= 90) || (code >= 97 && code <= 122);
      const digit = code >= 48 && code <= 57;
      if (!alpha && !digit && character !== "-") return false;
    }
    return true;
  });
}

function errorName(error: unknown): string {
  return error instanceof Error ? error.name : "UnknownError";
}

function log(stage: string, fields: Record<string, string | number>): void {
  console.log(JSON.stringify({ service: "pliegors-email", stage, ...fields }));
}
