import { describe, expect, it } from "vitest";
import { compactId, formatTimestamp, lifecycleTone, riskTone, statusText, timestampTitle } from "./formatters";

describe("operational formatters", () => {
  it("uses honest lifecycle tones without treating unreached work as failed", () => {
    expect(lifecycleTone("completed")).toBe("healthy");
    expect(lifecycleTone("executing")).toBe("running");
    expect(lifecycleTone("blocked")).toBe("blocked");
    expect(lifecycleTone("unreached")).toBe("future");
  });

  it("formats durable identifiers and labels compactly", () => {
    expect(compactId(undefined)).toBe("not scoped");
    expect(compactId("witem_12345678901234567890")).toBe("witem_1234...67890");
    expect(statusText("approval_required")).toBe("Approval Required");
    expect(riskTone("critical")).toBe("high");
  });

  it("uses relative age with a copyable absolute timestamp", () => {
    const now = Date.now();
    expect(formatTimestamp(String(now - 30_000))).toBe("just now");
    expect(formatTimestamp(String(now - 120_000))).toBe("2m ago");
    expect(timestampTitle(String(now))).not.toBe("Unknown time");
  });
});
