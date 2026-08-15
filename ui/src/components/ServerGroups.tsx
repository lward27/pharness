import { useState } from "react";
import { CaretDown, CaretRight, Stack } from "@phosphor-icons/react";
import { compactId, statusText } from "../lib/formatters";
import { StatusPill } from "./Operational";

type GroupMember = { id: string; label: string };
type ServerGroup = {
  key: string;
  title: string;
  resource: string;
  status: string;
  count: number;
  members: GroupMember[];
};

type ServerGroupsProps = {
  label: string;
  groups?: ServerGroup[];
  onOpen: (id: string) => void;
};

export function ServerGroups({ label, groups = [], onOpen }: ServerGroupsProps) {
  const [expanded, setExpanded] = useState<Record<string, boolean>>({});
  const repeated = groups.filter((group) => group.count > 1);
  if (repeated.length === 0) return null;

  return (
    <section className="server-groups" aria-label={`${label} groups`}>
      <header><Stack size={16} /><strong>Repeated {label}</strong><span>Server-calculated groups</span></header>
      {repeated.map((group) => {
        const open = Boolean(expanded[group.key]);
        return (
          <div className="server-group" key={group.key}>
            <button type="button" onClick={() => setExpanded((current) => ({ ...current, [group.key]: !open }))} aria-expanded={open}>
              {open ? <CaretDown size={15} /> : <CaretRight size={15} />}
              <span><strong>{group.title}</strong><small>{group.resource || "unscoped"}</small></span>
              <StatusPill tone={group.status === "blocked" || group.status === "failed" ? "blocked" : "pending"}>{statusText(group.status)}</StatusPill>
              <em>{group.count} records</em>
            </button>
            {open ? <div className="server-group-members">
              {group.members.map((member) => <button type="button" key={member.id} title={member.id} onClick={() => onOpen(member.id)}>{compactId(member.id)} <span>{member.label}</span></button>)}
            </div> : null}
          </div>
        );
      })}
    </section>
  );
}
