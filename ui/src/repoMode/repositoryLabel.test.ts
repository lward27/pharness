import { describe, expect, it } from "vitest";
import { repositoryLabel } from "./presentation";

describe("registered Repository display identity", () => {
  it("uses the registered owner/name when the provider ID is numeric", () => {
    expect(repositoryLabel({canonical_url:"https://github.com/lward27/yfinance_wrapper.git",external_id:"668351604"})).toBe("lward27/yfinance_wrapper");
    expect(repositoryLabel({canonical_url:"https://github.com/lward27/finance-frontend",provider_repository_id:"123456"})).toBe("lward27/finance-frontend");
  });
  it("retains historical identity and never changes mutation IDs", () => {
    const repository={id:"repo_123",external_id:"example/repo"};
    expect(repositoryLabel(repository)).toBe("example/repo");
    expect(repository.id).toBe("repo_123");
    expect(repositoryLabel(null,"repo_456")).toBe("repo_456");
    expect(repositoryLabel()).toBe("Unavailable");
  });
  it("does not display credentials or treat an arbitrary URL as a canonical GitHub identity", () => {
    for(const canonical_url of ["https://token@github.com/owner/repo.git","https://github.com.evil.test/owner/repo","https://github.com/owner/repo?token=secret","not a URL"]) {
      expect(repositoryLabel({canonical_url,id:"repo_123"})).toBe("repo_123");
    }
  });
});
