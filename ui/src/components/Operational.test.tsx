import { render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import { EmptyState, IconButton, ReviewItem, StatusPill } from "./Operational";

describe("shared operator components", () => {
  it("renders explicit status, evidence, and empty states", () => {
    render(<><StatusPill tone="pending">Pending</StatusPill><ReviewItem label="Boundary" value="Awaiting gate" /><EmptyState title="No WorkItems" body="Nothing durable has been submitted." /></>);
    expect(screen.getByText("Pending")).toHaveClass("pill-pending");
    expect(screen.getByText("Boundary")).toBeVisible();
    expect(screen.getByText("No WorkItems")).toBeVisible();
  });

  it("labels icon-only controls for operators and assistive technology", () => {
    render(<IconButton label="Refresh">R</IconButton>);
    expect(screen.getByRole("button", { name: "Refresh" })).toHaveAttribute("title", "Refresh");
  });
});
