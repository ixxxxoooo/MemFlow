// @author ixxxxoooo
// 轻量线性图标集

import type { ReactNode, SVGProps } from "react";

function I({ children, size = 16, ...rest }: SVGProps<SVGSVGElement> & { size?: number; children: ReactNode }) {
  return (
    <svg width={size} height={size} viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.8"
      strokeLinecap="round" strokeLinejoin="round" {...rest}>
      {children}
    </svg>
  );
}
type P = { size?: number; className?: string };

export const ChatIcon = (p: P) => <I {...p}><path d="M21 12a8 8 0 0 1-11.6 7.1L4 20l1-4.6A8 8 0 1 1 21 12Z" /></I>;
export const TimelineIcon = (p: P) => <I {...p}><circle cx="6" cy="6" r="2" /><circle cx="6" cy="18" r="2" /><path d="M6 8v8M11 6h9M11 18h9M11 12h9" /></I>;
export const LibraryIcon = (p: P) => <I {...p}><path d="M4 5a2 2 0 0 1 2-2h12v16H6a2 2 0 0 0-2 2V5Z" /><path d="M4 21h14M8 7h6M8 11h6" /></I>;
export const SettingsIcon = (p: P) => <I {...p}><circle cx="12" cy="12" r="3" /><path d="M19.4 15a1.7 1.7 0 0 0 .3 1.8l.1.1a2 2 0 1 1-2.8 2.8l-.1-.1a1.7 1.7 0 0 0-1.8-.3 1.7 1.7 0 0 0-1 1.5V21a2 2 0 1 1-4 0v-.1a1.7 1.7 0 0 0-1-1.5 1.7 1.7 0 0 0-1.8.3l-.1.1a2 2 0 1 1-2.8-2.8l.1-.1a1.7 1.7 0 0 0 .3-1.8 1.7 1.7 0 0 0-1.5-1H3a2 2 0 1 1 0-4h.1a1.7 1.7 0 0 0 1.5-1 1.7 1.7 0 0 0-.3-1.8l-.1-.1a2 2 0 1 1 2.8-2.8l.1.1a1.7 1.7 0 0 0 1.8.3h0a1.7 1.7 0 0 0 1-1.5V3a2 2 0 1 1 4 0v.1a1.7 1.7 0 0 0 1 1.5h0a1.7 1.7 0 0 0 1.8-.3l.1-.1a2 2 0 1 1 2.8 2.8l-.1.1a1.7 1.7 0 0 0-.3 1.8v0a1.7 1.7 0 0 0 1.5 1H21a2 2 0 1 1 0 4h-.1a1.7 1.7 0 0 0-1.5 1Z" /></I>;
export const SendIcon = (p: P) => <I {...p}><path d="M12 19V5M5 12l7-7 7 7" /></I>;
export const StopIcon = (p: P) => <I {...p}><rect x="6" y="6" width="12" height="12" rx="2" fill="currentColor" /></I>;
export const PlusIcon = (p: P) => <I {...p}><path d="M12 5v14M5 12h14" /></I>;
export const HistoryIcon = (p: P) => <I {...p}><path d="M3 12a9 9 0 1 0 3-6.7L3 8" /><path d="M3 3v5h5M12 7v5l3 2" /></I>;
export const TrashIcon = (p: P) => <I {...p}><path d="M4 7h16M10 11v6M14 11v6M6 7l1 13h10l1-13M9 7V4h6v3" /></I>;
export const RefreshIcon = (p: P) => <I {...p}><path d="M21 12a9 9 0 0 1-15.5 6.2M3 12A9 9 0 0 1 18.5 5.8M18 2v4h-4M6 22v-4h4" /></I>;
export const CloseIcon = (p: P) => <I {...p}><path d="M6 6l12 12M18 6 6 18" /></I>;
export const UploadIcon = (p: P) => <I {...p}><path d="M12 16V4M7 9l5-5 5 5M4 20h16" /></I>;
export const CopyIcon = (p: P) => <I {...p}><rect x="9" y="9" width="11" height="11" rx="2" /><path d="M5 15V6a2 2 0 0 1 2-2h9" /></I>;
export const ZoomIcon = (p: P) => <I {...p}><circle cx="11" cy="11" r="7" /><path d="m20 20-4-4M11 8v6M8 11h6" /></I>;
export const CodeIcon = (p: P) => <I {...p}><path d="m8 7-5 5 5 5M16 7l5 5-5 5M14 4l-4 16" /></I>;
export const DownloadIcon = (p: P) => <I {...p}><path d="M12 4v12M7 11l5 5 5-5M4 20h16" /></I>;
export const SparkleIcon = (p: P) => <I {...p}><path d="M12 3c.6 4.6 2.4 6.4 7 7-4.6.6-6.4 2.4-7 7-.6-4.6-2.4-6.4-7-7 4.6-.6 6.4-2.4 7-7ZM19 16c.3 2 1 2.700 3 3-2 .3-2.700 1-3 3-.3-2-1-2.700-3-3 2-.3 2.700-1 3-3Z" /></I>;
export const ImageIcon = (p: P) => <I {...p}><rect x="3" y="4" width="18" height="16" rx="2" /><circle cx="9" cy="10" r="2" /><path d="m21 16-5-5-9 9" /></I>;
export const FileIcon = (p: P) => <I {...p}><path d="M14 3H7a2 2 0 0 0-2 2v14a2 2 0 0 0 2 2h10a2 2 0 0 0 2-2V8l-5-5Z" /><path d="M14 3v5h5M9 13h6M9 17h6" /></I>;
export const NoteIcon = (p: P) => <I {...p}><path d="M4 4h16v12l-4 4H4V4Z" /><path d="M16 20v-4h4M8 9h8M8 13h5" /></I>;
export const SearchIcon = (p: P) => <I {...p}><circle cx="11" cy="11" r="7" /><path d="m20 20-4-4" /></I>;
export const CheckIcon = (p: P) => <I {...p}><path d="m5 12 5 5 9-10" /></I>;
export const AlertIcon = (p: P) => <I {...p}><path d="M12 3 2 20h20L12 3ZM12 10v4M12 17.500v.01" /></I>;
export const SidebarRightIcon = (p: P) => <I {...p}><rect x="3" y="4" width="18" height="16" rx="2" /><path d="M15 4v16" /></I>;
export const EditIcon = (p: P) => <I {...p}><path d="M4 20h4L19 9l-4-4L4 16v4ZM13.500 6.500l4 4" /></I>;
export const ChevronDownIcon = (p: P) => <I {...p}><path d="m6 9 6 6 6-6" /></I>;
export const FolderIcon = (p: P) => <I {...p}><path d="M3 7a2 2 0 0 1 2-2h4l2 2h8a2 2 0 0 1 2 2v8a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2V7Z" /></I>;
export const MoveIcon = (p: P) => <I {...p}><path d="M3 7a2 2 0 0 1 2-2h4l2 2h8a2 2 0 0 1 2 2v2M3 7v10a2 2 0 0 0 2 2h8M16 15h6m0 0-3-3m3 3-3 3" /></I>;
export const AtIcon = (p: P) => <I {...p}><circle cx="12" cy="12" r="4" /><path d="M16 8v5a3 3 0 0 0 6 0v-1a10 10 0 1 0-4 8" /></I>;
export const InfoIcon = (p: P) => <I {...p}><circle cx="12" cy="12" r="9" /><path d="M12 11v5M12 8h.01" /></I>;
export const ChevronLeftIcon = (p: P) => <I {...p}><path d="m15 6-6 6 6 6" /></I>;
export const ChevronRightIcon = (p: P) => <I {...p}><path d="m9 6 6 6-6 6" /></I>;
export const EyeIcon = (p: P) => <I {...p}><path d="M2 12s3.500-7 10-7 10 7 10 7-3.500 7-10 7S2 12 2 12Z" /><circle cx="12" cy="12" r="3" /></I>;
export const EyeOffIcon = (p: P) => <I {...p}><path d="M3 3l18 18M10.600 6.100A10 10 0 0 1 12 5c6.500 0 10 7 10 7a17 17 0 0 1-3.200 4.200M6.600 6.600C3.800 8.400 2 12 2 12s3.500 7 10 7a9.500 9.500 0 0 0 4.200-1M9.900 9.900a3 3 0 0 0 4.200 4.200" /></I>;
