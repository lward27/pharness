import { act, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { SearchDialog } from "./RepoModeApp";

describe("resource search", () => {
  afterEach(() => vi.unstubAllGlobals());
  it("ignores an old query even when its aborted response arrives last", async () => {
    const pending: Array<(value: Response) => void> = [];
    const fetch = vi.fn(() => new Promise<Response>(resolve => pending.push(resolve)));
    vi.stubGlobal("fetch", fetch);
    render(<SearchDialog onClose={() => {}} />);
    const input = screen.getByRole("textbox");
    fireEvent.change(input, {target:{value:"Market"}});
    await waitFor(() => expect(fetch).toHaveBeenCalledTimes(1));
    fireEvent.change(input, {target:{value:"Finance"}});
    await waitFor(() => expect(fetch).toHaveBeenCalledTimes(2));
    const result = (label: string) => new Response(JSON.stringify({results:[{kind:"product",id:label,label,status:"active"}]}));
    await act(async () => pending[1](result("Current Finance")));
    expect(screen.getByRole("button", {name:/Current Finance/})).toBeInTheDocument();
    await act(async () => pending[0](result("Old Market")));
    expect(screen.queryByRole("button", {name:/Old Market/})).not.toBeInTheDocument();
    expect(screen.getByRole("button", {name:/Current Finance/})).toBeInTheDocument();
  });
});
