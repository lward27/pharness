type ListPaginationProps = {
  label: string;
  count?: number;
  visibleCount: number;
  limit: number;
  offset: number;
  onOffsetChange: (offset: number) => void;
};

export function ListPagination({ label, count, visibleCount, limit, offset, onOffsetChange }: ListPaginationProps) {
  const total = typeof count === "number" && Number.isSafeInteger(count) && count >= 0 ? count : undefined;
  const lastPage = total === undefined ? offset : Math.max(0, Math.ceil(total / limit) - 1) * limit;
  const previous = Math.min(Math.max(0, offset - limit), lastPage);
  const hasPages = offset > 0 || (total !== undefined && total > limit);
  const summary = total === undefined
    ? `${visibleCount} on this page · Total unavailable`
    : visibleCount === 0
      ? total === 0 ? "0 matching" : `No results on this page · ${total} matching`
      : `${offset + 1}–${offset + visibleCount} of ${total}`;

  return <nav className="repo-pagination" aria-label={`${label} pages`}>
    {hasPages ? <button type="button" disabled={offset === 0} onClick={() => onOffsetChange(previous)}>Previous</button> : null}
    <span>{summary}</span>
    {hasPages ? <button type="button" disabled={total === undefined || offset + limit >= total} onClick={() => onOffsetChange(offset + limit)}>Next</button> : null}
  </nav>;
}
