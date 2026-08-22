import {
  CheckCircle,
  Clock,
  GitBranch,
  GitPullRequest,
  Pulse,
  RocketLaunch,
  ShieldCheck,
  Warning,
} from "@phosphor-icons/react";
import { buildDeliveryWorkspace, type DeliveryEvidenceStatus, type DeliveryStageModel } from "../lib/deliveryWorkspace";
import { compactId, statusText } from "../lib/formatters";
import { StatusPill } from "./Operational";

type DeliveryWorkspaceProps = {
  flow: any;
  item: any;
  onOpenResource: (resource: any) => void;
};

const stageIcons = {
  source: GitBranch,
  build: RocketLaunch,
  gitops: GitPullRequest,
  deploy: Pulse,
  verify: CheckCircle,
};

function statusTone(status: DeliveryEvidenceStatus) {
  if (status === "complete") return "healthy";
  if (status === "active") return "running";
  if (status === "blocked") return "blocked";
  if (status === "waiting") return "pending";
  return "future";
}

function evidenceIcon(status: string) {
  if (status === "passed") return <CheckCircle size={17} weight="fill" />;
  if (status === "failed") return <Warning size={17} weight="fill" />;
  return <Clock size={17} />;
}

function Stage({ stage, index, onOpenResource }: { stage: DeliveryStageModel; index: number; onOpenResource: (resource: any) => void }) {
  const Icon = stageIcons[stage.key];
  const disagrees = stage.status === "complete" && stage.controllerStatus !== "complete";
  return <li className={`release-stage is-${stage.status}`}>
    <div className="release-stage-marker"><span>{index + 1}</span><Icon size={19} /></div>
    <article>
      <header>
        <div><span className="eyebrow">Stage {index + 1}</span><h3>{stage.label}</h3></div>
        <div className="release-stage-status"><StatusPill tone={statusTone(stage.status)}>{statusText(stage.status)}</StatusPill>{disagrees ? <span className="controller-stage-status">Controller: {statusText(stage.controllerStatus)}</span> : null}</div>
      </header>
      <p className="release-stage-summary">{stage.summary}</p>
      {stage.checkpoint ? <div className={`release-checkpoint is-${stage.status}`}><strong>{stage.checkpoint}</strong>{stage.status === "waiting" ? <span>Human boundary</span> : null}</div> : null}
      <dl className="release-stage-facts">
        {stage.facts.map((fact) => <div key={`${stage.key}-${fact.label}`} className={fact.tone ? `tone-${fact.tone}` : undefined}><dt>{fact.label}</dt><dd title={fact.value}>{fact.value}</dd></div>)}
      </dl>
      {stage.evidence.length ? <div className="release-stage-evidence" aria-label={`${stage.label} evidence`}>
        {stage.evidence.map((entry) => <div className={`is-${entry.status}`} key={`${stage.key}-${entry.label}`}>
          {evidenceIcon(entry.status)}<span><strong>{statusText(entry.label)}</strong><small title={entry.detail}>{entry.detail}</small></span>
        </div>)}
      </div> : null}
      {stage.resources.length ? <div className="release-stage-resources">
        {stage.resources.map((entry) => <button type="button" key={`${stage.key}-${entry.id}`} title={entry.id} onClick={() => onOpenResource(entry)}>{entry.label}<span>{compactId(entry.id)}</span></button>)}
      </div> : <small className="release-stage-empty">No durable stage resource has been created.</small>}
    </article>
  </li>;
}

export function DeliveryWorkspace({ flow, item, onOpenResource }: DeliveryWorkspaceProps) {
  const model = buildDeliveryWorkspace(flow, item);
  const guardrails = model.guardrails;
  return <section className="release-workspace" aria-label="Delivery and release workspace">
    <header className="release-workspace-heading">
      <div><span className="eyebrow">Delivery runway</span><h2>Source to verified release</h2><p>Each external system owns one stage, one boundary, and its durable evidence.</p></div>
      <strong>{model.completedStages}/5 stages evidenced</strong>
    </header>

    <section className="release-guardrails" aria-label="Production release guardrails">
      <div className="release-guardrail-title"><ShieldCheck size={22} weight="fill" /><span><strong>Protected deployment context</strong><small>Visible through deploy and verify</small></span></div>
      <dl>
        <div><dt>Exact target</dt><dd>{guardrails.target}<small>Argo · {guardrails.argoApplication}</small></dd></div>
        <div className={`is-${guardrails.productionWindowStatus}`}><dt>Production window</dt><dd>{guardrails.productionWindow}</dd></div>
        <div className={`is-${guardrails.digestEqualityStatus}`}><dt>Digest contract</dt><dd>{guardrails.digestEquality}<small>Desired digest</small><code title={guardrails.desiredDigest}>{guardrails.desiredDigest}</code><small>Reported current digest</small><code title={guardrails.currentDigest}>{guardrails.currentDigest}</code></dd></div>
        <div><dt>Rollback baseline</dt><dd><code title={guardrails.baselineDigest}>{guardrails.baselineDigest}</code><small>Owner · {guardrails.rollbackOwner}</small><small>Intent · {statusText(guardrails.rollbackStatus)}</small></dd></div>
      </dl>
    </section>

    <ol className="release-timeline">
      {model.stages.map((stage, index) => <Stage key={stage.key} stage={stage} index={index} onOpenResource={onOpenResource} />)}
    </ol>
  </section>;
}
