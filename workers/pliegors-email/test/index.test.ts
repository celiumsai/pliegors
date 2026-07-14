import { beforeEach, describe, expect, it, vi } from "vitest";
import { autoReplySkipReason, processEmail } from "../src/handler";

type TestMessage = ForwardableEmailMessage & {
  forward: ReturnType<typeof vi.fn>;
  reply: ReturnType<typeof vi.fn>;
  setReject: ReturnType<typeof vi.fn>;
};

describe("PliegoRS email Worker", () => {
  beforeEach(() => {
    vi.spyOn(console, "log").mockImplementation(() => undefined);
  });

  it("forwards a human message before sending one acknowledgement", async () => {
    const message = testMessage();
    await processEmail(message, { FORWARD_TO: "team@example.com" });

    expect(message.forward).toHaveBeenCalledOnce();
    expect(message.forward).toHaveBeenCalledWith("team@example.com", expect.any(Headers));
    expect(message.reply).toHaveBeenCalledOnce();
    expect(message.forward.mock.invocationCallOrder[0]).toBeLessThan(
      message.reply.mock.invocationCallOrder[0],
    );
  });

  it("forwards automated mail without replying", async () => {
    const message = testMessage({
      from: "mailer-daemon@example.com",
      headers: new Headers({ "Auto-Submitted": "auto-generated" }),
    });
    await processEmail(message, { FORWARD_TO: "team@example.com" });

    expect(message.forward).toHaveBeenCalledOnce();
    expect(message.reply).not.toHaveBeenCalled();
  });

  it("rejects recipients outside the literal public mailbox", async () => {
    const message = testMessage({ to: "other@pliegors.dev" });
    await processEmail(message, { FORWARD_TO: "team@example.com" });

    expect(message.setReject).toHaveBeenCalledOnce();
    expect(message.forward).not.toHaveBeenCalled();
    expect(message.reply).not.toHaveBeenCalled();
  });

  it("keeps a successfully forwarded message when the platform rejects its reply", async () => {
    const message = testMessage();
    message.reply.mockRejectedValueOnce(new Error("DMARC rejected"));
    await expect(
      processEmail(message, { FORWARD_TO: "team@example.com" }),
    ).resolves.toBeUndefined();

    expect(message.forward).toHaveBeenCalledOnce();
    expect(message.reply).toHaveBeenCalledOnce();
  });

  it("fails closed when the forwarding destination is not configured", async () => {
    const message = testMessage();
    await expect(processEmail(message, { FORWARD_TO: "" })).rejects.toThrow("FORWARD_TO");
    expect(message.forward).not.toHaveBeenCalled();
  });

  it("detects mailing lists and reference floods", () => {
    const list = testMessage({ headers: new Headers({ "List-Id": "project.example" }) });
    expect(autoReplySkipReason(list)).toBe("mailing-list");

    const references = Array.from({ length: 101 }, (_, index) => `<${index}@example.com>`).join(" ");
    const flood = testMessage({ headers: new Headers({ References: references }) });
    expect(autoReplySkipReason(flood)).toBe("reference-limit");
  });
});

function testMessage(overrides: Partial<TestMessage> = {}): TestMessage {
  return {
    from: "person@example.com",
    to: "hello@pliegors.dev",
    headers: new Headers({
      "Message-ID": "<message@example.com>",
      Subject: "Framework question",
    }),
    raw: new ReadableStream(),
    rawSize: 512,
    canBeForwarded: true,
    forward: vi.fn().mockResolvedValue(undefined),
    reply: vi.fn().mockResolvedValue(undefined),
    setReject: vi.fn(),
    ...overrides,
  } as unknown as TestMessage;
}
