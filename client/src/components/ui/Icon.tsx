// 自作の最小アイコンセット (外部依存を増やさないための手描き SVG)。
// stroke ベースで統一し、サイズ・線幅だけをプロパティで揺らす。
import type { SVGProps } from "react";

export type IconName =
  | "home"
  | "book"
  | "settings"
  | "info"
  | "mic"
  | "check"
  | "copy"
  | "alert-triangle"
  | "chevron-right"
  | "chevron-down"
  | "x"
  | "plus"
  | "refresh"
  | "external-link"
  | "wifi-off"
  | "wifi"
  | "loader"
  | "search"
  | "fingerprint"
  | "keyboard";

const PATHS: Record<IconName, string> = {
  home: "M3 10.5 12 3l9 7.5M5 9.5V20a1 1 0 0 0 1 1h4v-6h4v6h4a1 1 0 0 0 1-1V9.5",
  book: "M4 5a2 2 0 0 1 2-2h13v16H6a2 2 0 0 0-2 2V5Zm2 14a2 2 0 0 0 0 4h13v-4",
  settings:
    "M12 15.5a3.5 3.5 0 1 0 0-7 3.5 3.5 0 0 0 0 7Zm7.4-3.5a7.6 7.6 0 0 0-.14-1.44l1.9-1.48-2-3.46-2.24.76a7.7 7.7 0 0 0-1.24-.72L15.3 3.5h-4l-.4 2.16a7.7 7.7 0 0 0-1.24.72l-2.24-.76-2 3.46 1.9 1.48A7.6 7.6 0 0 0 7.18 12c0 .48.05.96.14 1.44l-1.9 1.48 2 3.46 2.24-.76c.38.28.8.52 1.24.72l.4 2.16h4l.4-2.16c.44-.2.86-.44 1.24-.72l2.24.76 2-3.46-1.9-1.48c.09-.48.14-.96.14-1.44Z",
  info: "M12 21a9 9 0 1 0 0-18 9 9 0 0 0 0 18Zm0-11v6M12 7.5v.01",
  mic: "M12 15a3.5 3.5 0 0 0 3.5-3.5V6a3.5 3.5 0 0 0-7 0v5.5A3.5 3.5 0 0 0 12 15Zm-6.5-3.5a6.5 6.5 0 0 0 13 0M12 18v3",
  check: "M4 12.5 9 17.5 20 6.5",
  copy: "M9 9V5a2 2 0 0 1 2-2h8a2 2 0 0 1 2 2v8a2 2 0 0 1-2 2h-4M5 9h8a2 2 0 0 1 2 2v8a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2v-8a2 2 0 0 1 2-2Z",
  "alert-triangle": "M12 4 2.5 20h19L12 4Zm0 6.5v4.5M12 18v.01",
  "chevron-right": "M9 5l7 7-7 7",
  "chevron-down": "M5 9l7 7 7-7",
  x: "M5 5l14 14M19 5 5 19",
  plus: "M12 5v14M5 12h14",
  refresh: "M4 12a8 8 0 0 1 14.6-4.6M20 12a8 8 0 0 1-14.6 4.6M17 4v4h-4M7 20v-4h4",
  "external-link": "M14 4h6v6M20 4 10 14M6 4H5a1 1 0 0 0-1 1v14a1 1 0 0 0 1 1h14a1 1 0 0 0 1-1v-1",
  "wifi-off": "M3 3l18 18M8.5 16.5a5 5 0 0 1 7 0M5.5 12.5a10 10 0 0 1 4-2.4M18.5 12.5a10 10 0 0 0-2.6-2M12 20v.01",
  wifi: "M5 12.5a10 10 0 0 1 14 0M8.5 16a5 5 0 0 1 7 0M12 20v.01",
  loader: "M12 3v3M12 18v3M4.2 4.2l2.1 2.1M17.7 17.7l2.1 2.1M3 12h3M18 12h3M4.2 19.8l2.1-2.1M17.7 6.3l2.1-2.1",
  search: "M11 18a7 7 0 1 0 0-14 7 7 0 0 0 0 14Zm9 3-5-5",
  fingerprint:
    "M12 3a9 9 0 0 0-9 9m18 0a9 9 0 0 0-4-7.5M12 21a9 9 0 0 0 6.7-3M8 8.5a4 4 0 0 1 8 0v3a8 8 0 0 1-2 5.3M8 15a7 7 0 0 1-1-3.6v-3a5 5 0 0 1 .3-1.7",
  keyboard:
    "M3 7h18a1 1 0 0 1 1 1v8a1 1 0 0 1-1 1H3a1 1 0 0 1-1-1V8a1 1 0 0 1 1-1Zm2.5 3h1M9 10h1m3 0h1m3 0h1M5.5 13h1m3 0h5m3 0h1M8 16h8",
};

interface IconProps extends Omit<SVGProps<SVGSVGElement>, "name"> {
  name: IconName;
  size?: number;
}

export default function Icon({ name, size = 16, ...rest }: IconProps) {
  return (
    <svg
      width={size}
      height={size}
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      strokeWidth={1.8}
      strokeLinecap="round"
      strokeLinejoin="round"
      aria-hidden="true"
      focusable="false"
      {...rest}
    >
      <path d={PATHS[name]} />
    </svg>
  );
}
