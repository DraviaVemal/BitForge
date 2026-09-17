import { useMemo, useState, type ReactNode } from "react";

type Align = "left" | "right" | "center";
type ColumnType = "text" | "number";
type SortDirection = "asc" | "desc";

export type Column<Row> = {
  key: string;
  header: ReactNode;
  type?: ColumnType;
  sortable?: boolean;
  filterable?: boolean;
  align?: Align;
  width?: string;
  className?: string;
  render?: (row: Row) => ReactNode;
  value?: (row: Row) => string | number;
};

type DataTableProps<Row> = {
  columns: Column<Row>[];
  rows: Row[];
  rowKey: (row: Row, index: number) => string | number;
  rowClassName?: (row: Row) => string;
  onRowClick?: (row: Row) => void;
  initialSort?: { key: string; direction: SortDirection };
  empty?: ReactNode;
  className?: string;
};

function isSortable<Row>(column: Column<Row>): boolean {
  return column.sortable ?? column.type === "number";
}

function isFilterable<Row>(column: Column<Row>): boolean {
  return column.filterable ?? column.type === "text";
}

function cellValue<Row>(column: Column<Row>, row: Row): string | number {
  if (column.value) return column.value(row);
  const raw = (row as Record<string, unknown>)[column.key];
  if (typeof raw === "number") return raw;
  return raw == null ? "" : String(raw);
}

export default function DataTable<Row>({
  columns,
  rows,
  rowKey,
  rowClassName,
  onRowClick,
  initialSort,
  empty,
  className,
}: DataTableProps<Row>) {
  const [sort, setSort] = useState<{ key: string; direction: SortDirection } | null>(
    initialSort ?? null,
  );
  const [filters, setFilters] = useState<Record<string, string>>({});

  const hasFilters = columns.some((column) => isFilterable(column));

  const toggleSort = (column: Column<Row>) => {
    if (!isSortable(column)) return;
    setSort((current) => {
      if (!current || current.key !== column.key) return { key: column.key, direction: "asc" };
      if (current.direction === "asc") return { key: column.key, direction: "desc" };
      return null;
    });
  };

  const visibleRows = useMemo(() => {
    const activeFilters = Object.entries(filters).filter(([, value]) => value.trim() !== "");
    let result = rows.filter((row) =>
      activeFilters.every(([key, needle]) => {
        const column = columns.find((entry) => entry.key === key);
        if (!column) return true;
        return String(cellValue(column, row)).toLowerCase().includes(needle.trim().toLowerCase());
      }),
    );

    if (sort) {
      const column = columns.find((entry) => entry.key === sort.key);
      if (column) {
        const factor = sort.direction === "asc" ? 1 : -1;
        result = [...result].sort((left, right) => {
          const leftValue = cellValue(column, left);
          const rightValue = cellValue(column, right);
          if (typeof leftValue === "number" && typeof rightValue === "number") {
            return (leftValue - rightValue) * factor;
          }
          return String(leftValue).localeCompare(String(rightValue)) * factor;
        });
      }
    }

    return result;
  }, [rows, columns, filters, sort]);

  return (
    <div className={`data-table-wrap ${className ?? ""}`}>
      <table className="table data-table">
        <thead>
          <tr>
            {columns.map((column) => {
              const sortable = isSortable(column);
              const activeSort = sort?.key === column.key ? sort.direction : null;
              return (
                <th
                  key={column.key}
                  style={{ width: column.width, textAlign: column.align }}
                  className={sortable ? "sortable" : undefined}
                >
                  <button
                    type="button"
                    className="col-head"
                    onClick={() => toggleSort(column)}
                    disabled={!sortable}
                  >
                    <span>{column.header}</span>
                    {sortable && (
                      <span className={`sort-indicator ${activeSort ?? ""}`}>
                        {activeSort === "asc" ? "▲" : activeSort === "desc" ? "▼" : "⇅"}
                      </span>
                    )}
                  </button>
                </th>
              );
            })}
          </tr>
          {hasFilters && (
            <tr className="filter-row">
              {columns.map((column) => (
                <th key={column.key}>
                  {isFilterable(column) && (
                    <input
                      className="col-filter"
                      value={filters[column.key] ?? ""}
                      placeholder="Filter…"
                      onChange={(event) =>
                        setFilters((current) => ({ ...current, [column.key]: event.target.value }))
                      }
                    />
                  )}
                </th>
              ))}
            </tr>
          )}
        </thead>
        <tbody>
          {visibleRows.map((row, index) => (
            <tr
              key={rowKey(row, index)}
              className={rowClassName?.(row)}
              onClick={onRowClick ? () => onRowClick(row) : undefined}
              style={onRowClick ? { cursor: "pointer" } : undefined}
            >
              {columns.map((column) => (
                <td
                  key={column.key}
                  className={column.className}
                  style={{ textAlign: column.align }}
                >
                  {column.render ? column.render(row) : cellValue(column, row)}
                </td>
              ))}
            </tr>
          ))}
        </tbody>
      </table>
      {visibleRows.length === 0 && (
        <p className="muted small data-table-empty">{empty ?? "No matching rows."}</p>
      )}
    </div>
  );
}
