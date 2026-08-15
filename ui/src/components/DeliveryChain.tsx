import {
  CheckCircle,
  FlowArrow,
  GitBranch,
  GitPullRequest,
  Pulse,
  RocketLaunch,
} from "@phosphor-icons/react";

type DeliveryChainProps = {
  segments?: any[];
  onOpenResource: (resource: any) => void;
};

type Stage = {
  key: string;
  label: string;
  icon: any;
  status: "complete" | "active" | "blocked" | "unreached";
  summary: string;
  resources: Array<{ label: string; id?: string; summary?: string; kind?: string }>;
};

function compactId(value?: string) {
  if (!value) return "not created";
  return value.length <= 20 ? value : `${value.slice(0, 11)}...${value.slice(-5)}`;
}

const icons: Record<string, any> = { source: GitBranch, build: RocketLaunch, gitops: GitPullRequest, deploy: Pulse, verify: CheckCircle };

export function DeliveryChain({ segments = [], onOpenResource }: DeliveryChainProps) {
  const stages: Stage[] = segments.map((segment) => ({
    ...segment,
    icon: icons[segment.key] ?? FlowArrow,
    status: segment.status,
  }));

  return (
    <section className="delivery-chain" aria-label="WorkItem delivery chain">
      <div className="delivery-chain-heading">
        <div>
          <span className="eyebrow">Delivery chain</span>
          <h2>Source to verification</h2>
        </div>
        <span>Server-backed durable evidence</span>
      </div>
      <div className="delivery-stages">
        {stages.map((stage) => {
          const Icon = stage.icon;
          return (
            <section className={`delivery-stage is-${stage.status}`} key={stage.key}>
              <header>
                <Icon size={19} />
                <strong>{stage.label}</strong>
                <span>{stage.status === "unreached" ? "Unreached" : stage.status}</span>
              </header>
              <p>{stage.summary}</p>
              <div>
                {stage.resources.filter((item) => item.id).map((item) => (
                  <button
                    key={`${stage.key}-${item.label}-${item.id}`}
                    type="button"
                    title={item.id}
                    onClick={() => onOpenResource(item)}
                  >
                    {item.label} · {compactId(item.id)}
                  </button>
                ))}
                {!stage.resources.some((item) => item.id) ? <small>Not created</small> : null}
              </div>
            </section>
          );
        })}
      </div>
    </section>
  );
}
