import type { ReactNode } from "react";
import type { OutputMode, Status } from "../../types";
import Header from "./Header";
import Sidebar, { type ViewId } from "./Sidebar";
import styles from "./Shell.module.css";

interface ShellProps {
  view: ViewId;
  onChangeView: (view: ViewId) => void;
  status: Status | null;
  outputMode: OutputMode;
  children: ReactNode;
}

export default function Shell({ view, onChangeView, status, outputMode, children }: ShellProps) {
  return (
    <div className={styles.shell}>
      <Sidebar current={view} onChange={onChangeView} />
      <div className={styles.body}>
        <Header status={status} outputMode={outputMode} />
        <main className={styles.main}>{children}</main>
      </div>
    </div>
  );
}
