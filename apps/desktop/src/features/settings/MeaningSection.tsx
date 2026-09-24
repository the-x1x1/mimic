import { Button, Card, ProgressBar } from "@mimic/ui";
import { JOB_KINDS } from "@mimic/contracts";
import { useDownloadEncoder, useEncoder } from "@/hooks/useVoice";
import { useJobs } from "@/hooks/useJobs";
import { countOf } from "@/lib/format";

/**
 * What the engine says when a file is missing, cut short or changed: the
 * cases a fresh download mends. A build that cannot run an encoder, or one
 * that could not load it, is not mended by fetching the same files again.
 */
const MENDED_BY_DOWNLOADING = /has not been downloaded|is not the size|does not match/;

function megabytes(bytes: number): string {
  return `${Math.max(1, Math.round(bytes / 1_000_000))} MB`;
}

/**
 * Finding past replies by meaning. Without the encoder, the replies a draft
 * is shown are found by the words they share with what is being answered;
 * with it, by what they mean. It is downloaded only when asked, checked
 * against the digest this version pins, and runs on this computer.
 */
export function MeaningSection() {
  const encoder = useEncoder();
  const download = useDownloadEncoder();
  const jobs = useJobs(true);
  const e = encoder.data;
  if (!e?.offered) return null;
  const active = (jobs.data ?? []).filter((j) => j.status === "queued" || j.status === "running");
  const downloading = active.find((j) => j.type === JOB_KINDS.downloadEncoder);
  const reading = active.find((j) => j.type === JOB_KINDS.embedMessages);
  const name = e.offered.name;

  return (
    <Card title="Finding your past replies by meaning">
      <p>
        A draft is shown a few of your past replies to messages like the one you&rsquo;re answering.
        Without an encoder I find them by the words they share; with one, by what they mean &mdash;
        &ldquo;drinks on friday?&rdquo; finds &ldquo;pub friday?&rdquo;.
      </p>
      {e.inUse ? (
        <div className="stack gap-1">
          <p>
            {name} is reading your messages for meaning, on this computer.{" "}
            {`${countOf(e.done, "message")} of ${e.wanted.toLocaleString()} read.`}
          </p>
          {reading || e.done < e.wanted ? (
            <ProgressBar
              current={e.done}
              total={e.wanted}
              label={reading ? "Reading them now" : "The rest are read as mail comes in"}
            />
          ) : null}
        </div>
      ) : e.downloaded ? (
        <div className="stack gap-1">
          <p className="muted small">
            {name} is downloaded, but the engine isn&rsquo;t using it
            {e.reason ? `: ${e.reason}` : " — it isn't running"}.
          </p>
          {/* A file that no longer matches is fetched again; one that does is kept. */}
          {MENDED_BY_DOWNLOADING.test(e.reason ?? "") ? (
            <div className="row gap-2">
              <Button
                variant="ghost"
                size="sm"
                disabled={download.isPending || downloading !== undefined}
                onClick={() => download.mutate()}
              >
                {downloading ? "Downloading…" : "Download it again"}
              </Button>
            </div>
          ) : null}
        </div>
      ) : (
        <div className="stack gap-1">
          <div className="row gap-2">
            <Button
              variant="secondary"
              disabled={download.isPending || downloading !== undefined}
              onClick={() => download.mutate()}
            >
              {downloading ? "Downloading…" : `Download ${name} (${megabytes(e.offered.bytes)})`}
            </Button>
          </div>
          {downloading ? (
            <ProgressBar
              current={downloading.progressCurrent}
              total={downloading.progressTotal}
              label="Downloading"
            />
          ) : null}
        </div>
      )}
      <p className="muted small">
        {e.offered.description} It comes from Hugging Face ({e.offered.license}), only when you ask,
        and is used only while every file matches the digest this version of Mimic pins. It runs
        here: reading your messages for meaning sends nothing anywhere.
      </p>
    </Card>
  );
}
