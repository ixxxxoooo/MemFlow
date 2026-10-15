// @author ixxxxoooo
// 模型品牌图标:未识别时显示通用占位,黑色单色图标在深色主题下自动反相

import { BRAND_ICONS, brandOf, MONO_BRANDS } from "../../lib/brands";
import { SparkleIcon } from "./icons";

interface Props {
  model: string | null | undefined;
  preset?: string | null;
  provider?: string | null;
  size?: number;
  className?: string;
}

export function BrandIcon({ model, preset, provider, size = 16, className = "" }: Props) {
  const brand = brandOf(model, preset, provider);
  if (!brand) return <SparkleIcon size={size} className={`shrink-0 text-[var(--fg-dim)] ${className}`} />;
  return (
    <img
      src={BRAND_ICONS[brand]}
      alt=""
      width={size}
      height={size}
      draggable={false}
      className={`shrink-0 ${MONO_BRANDS.has(brand) ? "brand-mono" : ""} ${className}`}
    />
  );
}
