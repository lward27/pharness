import type { OperatorListFiltersValue } from "../components/Operational";

type OperationalRecord = {
  status?: string | null;
  origin?: string | null;
  created_by?: string | null;
  decided_by?: string | null;
  status_changed_by?: string | null;
};

type ScopeOptions = {
  actors?: string[];
  origins?: string[];
};

export function recordActor(record: OperationalRecord) {
  return record?.created_by ?? record?.decided_by ?? record?.status_changed_by ?? "";
}

export function recordOrigin(record: OperationalRecord) {
  return record?.origin ?? "legacy";
}

export function matchesOperationalFilters<T extends OperationalRecord>(
  record: T,
  filters: OperatorListFiltersValue,
  text: (record: T) => string,
) {
  const haystack = text(record).toLocaleLowerCase();
  return (!filters.search || haystack.includes(filters.search.trim().toLocaleLowerCase()))
    && (!filters.status || record.status === filters.status)
    && (!filters.actor || recordActor(record) === filters.actor)
    && (!filters.origin || recordOrigin(record) === filters.origin);
}

export function operationalFilterOptions<T extends OperationalRecord>(records: T[], scopeOptions: ScopeOptions = {}) {
  const from = (key: keyof ScopeOptions, fallback: (record: T) => string) => [
    ...new Set([...(scopeOptions[key] ?? []), ...records.map(fallback).filter(Boolean)]),
  ].sort();
  return {
    statuses: [...new Set(records.map((record) => record.status).filter(Boolean) as string[])].sort(),
    actors: from("actors", recordActor),
    origins: from("origins", recordOrigin),
  };
}
