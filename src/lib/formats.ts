// @author ygw
// 导入支持的文件格式:文件对话框过滤器、界面提示、拖拽提示的唯一来源。
// 必须与后端 `extract::classify` 保持一致(有单测对照 Rust 源码)。

export interface DocFormat {
  /** 展示名 */
  label: string;
  /** 扩展名(小写,不含点) */
  exts: string[];
  /** 解析说明 */
  note: string;
}

/** 文档类格式 */
export const DOC_FORMATS: DocFormat[] = [
  { label: "PDF", exts: ["pdf"], note: "文本层 + 扫描页本机 OCR(macOS)、表格还原为 Markdown、保留页码" },
  { label: "Word", exts: ["docx"], note: "标题层级、表格(含合并单元格)、内嵌图片" },
  { label: "PPT", exts: ["pptx"], note: "按幻灯片提取标题、正文、备注与图片" },
  { label: "Excel", exts: ["xlsx"], note: "每个工作表转 Markdown 表格,大表分块时重复表头" },
  { label: "CSV", exts: ["csv", "tsv"], note: "自动识别 UTF-8 / GBK / UTF-16 编码" },
  { label: "HTML", exts: ["html", "htm", "xhtml"], note: "提取正文、标题层级与表格,关联页内图片" },
  { label: "EPUB", exts: ["epub"], note: "按目录顺序提取章节与插图" },
  { label: "RTF", exts: ["rtf"], note: "提取文字(含中文代码页)" },
  { label: "Markdown", exts: ["md", "markdown"], note: "相对路径的本地图片会自动关联" },
  { label: "TXT", exts: ["txt"], note: "纯文本" },
];

/** 图片格式 */
export const IMAGE_FORMAT: DocFormat = {
  label: "图片",
  exts: ["png", "jpg", "jpeg", "webp", "gif", "bmp"],
  note: "本机 OCR 识别图中文字,可选视觉模型生成描述",
};

/** 文档扩展名(文件选择对话框用) */
export const DOC_EXTENSIONS: string[] = DOC_FORMATS.flatMap((f) => f.exts);

/** 文档 + 图片扩展名 */
export const ALL_EXTENSIONS: string[] = [...DOC_EXTENSIONS, ...IMAGE_FORMAT.exts];

/** 一行摘要:用于页头、空状态与拖拽遮罩 */
export const FORMATS_SUMMARY = "PDF · Word · PPT · Excel · CSV · HTML · EPUB · RTF · Markdown · TXT · 图片";

/** 不支持的格式说明 */
export const UNSUPPORTED_NOTE = "暂不支持旧版 .doc / .ppt / .xls(请先另存为 .docx / .pptx / .xlsx)。";

/** 导入文件夹的规则说明 */
export const FOLDER_NOTE = "导入文件夹会递归处理以上格式,并跳过隐藏文件与 node_modules、.git 等目录。";
