// @author ixxxxoooo
// 知识档案库:两级结构。第一级是知识库列表(增删改查),点击进入后才是库内文档。

import { useApp } from "../../lib/store";
import { KbDetailView } from "./KbDetailView";
import { KbListView } from "./KbListView";

export function DocsView() {
  const docsKb = useApp((s) => s.docsKb);
  return docsKb == null ? <KbListView /> : <KbDetailView kbId={docsKb} />;
}
