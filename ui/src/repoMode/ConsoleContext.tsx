import { createContext, useContext } from "react";
import { useResource } from "./useResource";

export const ConsoleContext = createContext<{ designOverhaul: boolean; overview?: ReturnType<typeof useResource<any>> }>({ designOverhaul: false });
export const useConsoleDesign = () => useContext(ConsoleContext).designOverhaul;
export function useOrganizationOverview() {
  const shared = useContext(ConsoleContext).overview;
  const local = useResource<any>(shared ? null : "/api/organization/overview", { pollMs: 20_000 });
  return shared || local;
}
