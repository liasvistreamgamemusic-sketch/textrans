import Icon from "../ui/Icon";
import Button from "../ui/Button";
import styles from "./AccessibilityWarning.module.css";

export default function AccessibilityWarning({ onOpenSettings }: { onOpenSettings: () => void }) {
  return (
    <div className={`${styles.card} reveal`} role="alert">
      <Icon name="alert-triangle" size={20} className={styles.icon} />
      <div className={styles.body}>
        <p className={styles.title}>アクセシビリティ権限が許可されていません</p>
        <p className={styles.desc}>
          入力欄へのテキスト挿入にはアクセシビリティ権限が必要です。許可しないと、認識結果はクリップボードに
          残るだけで自動挿入されません。
        </p>
        <div className={styles.action}>
          <Button variant="primary" size="sm" onClick={onOpenSettings}>
            <Icon name="external-link" size={13} />
            システム設定を開く
          </Button>
        </div>
      </div>
    </div>
  );
}
