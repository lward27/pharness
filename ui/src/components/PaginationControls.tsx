type PaginationControlsProps = {
  count: number;
  limit: number;
  offset: number;
  onOffsetChange: (offset: number) => void;
  label: string;
  loading?: boolean;
};

export function PaginationControls({ count, limit, offset, onOffsetChange, label, loading = false }: PaginationControlsProps) {
  if (count <= limit && offset === 0) return null;
  const start = count ? offset + 1 : 0;
  const end = Math.min(offset + limit, count);
  return <nav className="pagination-controls" aria-label={`${label} pagination`}>
    <span>{loading ? "Loading..." : `${start}-${end} of ${count}`}</span>
    <button type="button" onClick={() => onOffsetChange(Math.max(0, offset - limit))} disabled={loading || offset === 0}>Previous</button>
    <button type="button" onClick={() => onOffsetChange(offset + limit)} disabled={loading || offset + limit >= count}>Next</button>
  </nav>;
}
