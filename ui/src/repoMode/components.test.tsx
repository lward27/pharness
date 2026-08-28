import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { ActionDialog, type ServerAction } from "./components";

describe("Repo Mode action review", () => {
  afterEach(() => vi.unstubAllGlobals());

  it("clears the pending state and keeps the server blocker visible after a 409", async () => {
    const onApplied = vi.fn();
    const onClose = vi.fn();
    vi.stubGlobal("fetch", vi.fn(async () => new Response(
      JSON.stringify({ error: "remote workspace allowlist does not include finance-frontend" }),
      { status: 409, statusText: "Conflict", headers: { "content-type": "application/json" } },
    )));
    const action: ServerAction = {
      id: "complete_onboarding",
      lifecycle_stage: "onboarding",
      status: "available",
      effect_class: "internal",
      state_hash: "sha256:current-state",
    };

    render(<ActionDialog
      action={action}
      owner={{ kind: "Repository onboarding", id: "ronb_frontend" }}
      endpoint="/api/repository-onboardings/ronb_frontend/actions/complete_onboarding/execute"
      operatorName="lucas"
      onClose={onClose}
      onApplied={onApplied}
    />);

    fireEvent.change(screen.getByLabelText("Reason"), { target: { value: "Complete the reviewed onboarding" } });
    fireEvent.click(screen.getByRole("button", { name: "Confirm and apply" }));

    await waitFor(() => expect(screen.getByRole("alert")).toHaveTextContent("remote workspace allowlist does not include finance-frontend"));
    expect(screen.getByRole("button", { name: "Confirm and apply" })).toBeEnabled();
    expect(onApplied).toHaveBeenCalledTimes(1);
    expect(onClose).not.toHaveBeenCalled();
  });
});
