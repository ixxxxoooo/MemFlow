// @author ygw
//! 所有提示词与结构化输出 Schema 集中管理。修改提示词时需递增 PROMPT_VERSION,
//! 以便"重刷"能识别哪些记录由旧版提示词生成。

use crate::ollama::ChatMessage;
use serde::Deserialize;
use serde_json::{json, Value};

/// 提示词版本号
pub const PROMPT_VERSION: i64 = 1;

/// 文本/文档结构化提取结果
#[derive(Debug, Clone, Default, Deserialize)]
pub struct ExtractedMeta {
    /// 摘要
    #[serde(default)]
    pub summary: String,
    /// 分类
    #[serde(default)]
    pub category: String,
    /// 标签
    #[serde(default)]
    pub tags: Vec<String>,
    /// 实体(人名/项目/组织/产品)
    #[serde(default)]
    pub entities: Vec<String>,
}

/// 视觉解析结果
#[derive(Debug, Clone, Default, Deserialize)]
pub struct VisionResult {
    /// 画面描述
    #[serde(default)]
    pub description: String,
    /// 图中文字(OCR)
    #[serde(default)]
    pub ocr_text: String,
    /// 核心意图
    #[serde(default)]
    pub intent: String,
    /// 标签
    #[serde(default)]
    pub tags: Vec<String>,
    /// 分类
    #[serde(default)]
    pub category: String,
}

/// 结构化提取的 JSON Schema
pub fn extract_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "summary": {"type": "string"},
            "category": {"type": "string"},
            "tags": {"type": "array", "items": {"type": "string"}},
            "entities": {"type": "array", "items": {"type": "string"}}
        },
        "required": ["summary", "category", "tags", "entities"]
    })
}

/// 视觉解析的 JSON Schema
pub fn vision_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "description": {"type": "string"},
            "ocr_text": {"type": "string"},
            "intent": {"type": "string"},
            "tags": {"type": "array", "items": {"type": "string"}},
            "category": {"type": "string"}
        },
        "required": ["description", "ocr_text", "intent", "tags", "category"]
    })
}

/// 构造"记录/文档结构化提取"消息
///
/// 参数: kind 类型(note/document/image 说明);text 正文(过长需调用方先采样);time_label 记录时间文案;is_doc 是否文档
/// 返回: 消息列表
pub fn extract_messages(text: &str, time_label: &str, is_doc: bool) -> Vec<ChatMessage> {
    let summary_rule = if is_doc {
        "summary:200 字以内的全文摘要,概括主题、结论与关键信息"
    } else {
        "summary:60 字以内的一句话摘要,保留关键人物、数字与结论"
    };
    let system = format!(
        "你是个人知识库的归档助手。阅读用户的内容,只输出一个 JSON 对象,不要输出其他文字。\n\
         字段说明:\n\
         - {summary_rule}\n\
         - category:分类,2~6 个字,如 职场/项目、生活、学习、技术、财务、灵感、健康、人物 等,可自拟\n\
         - tags:2~5 个简短标签,不带 # 号,优先使用内容中出现的专有名词\n\
         - entities:内容中出现的人名、项目名、组织、产品名,没有则为空数组\n\
         所有字段使用与内容相同的语言(通常为简体中文),不要编造内容里没有的信息。"
    );
    let user = format!("记录时间:{time_label}\n\n内容:\n{text}");
    vec![
        ChatMessage::new("system", system),
        ChatMessage::new("user", user),
    ]
}

/// 构造"视觉解析"消息
///
/// 参数: caption 用户给图片的说明(可为空);image_b64 图片 base64
/// 返回: 消息列表
pub fn vision_messages(caption: &str, image_b64: String) -> Vec<ChatMessage> {
    let mut prompt = String::from(
        "请详细描述这张图片的内容,提取图中所有文字(OCR,保持原有顺序),分析其核心意图,并给出 3~5 个分类标签。\n\
         只输出 JSON,字段:description(画面描述)、ocr_text(图中文字,没有则为空字符串)、intent(核心意图,一句话)、tags(标签数组)、category(分类,2~6 个字)。使用简体中文。",
    );
    if !caption.trim().is_empty() {
        prompt.push_str(&format!("\n用户对这张图的说明:{caption}"));
    }
    let mut m = ChatMessage::new("user", prompt);
    m.images = Some(vec![image_b64]);
    vec![m]
}

/// 构造"PDF 页面转 Markdown"的视觉请求
///
/// 参数: page_png_b64 页面渲染图的 base64
/// 返回: 单条用户消息
pub fn pdf_page_messages(page_png_b64: String) -> Vec<ChatMessage> {
    let prompt = "把这一页文档转换为 Markdown,严格遵守:\n\
        1. 保留阅读顺序;标题用 # 层级表示;列表用 - 表示。\n\
        2. 表格必须输出为 Markdown 表格(含表头分隔行),单元格内容逐字抄录,合并单元格重复填写。\n\
        3. 逐字转录页面上所有文字,不要翻译、总结或补充说明。\n\
        4. 忽略页眉页脚和页码;图片/图表用一句话 [图:说明] 概括。\n\
        只输出 Markdown 正文,不要使用代码围栏。";
    let mut m = ChatMessage::new("user", prompt.to_string());
    m.images = Some(vec![page_png_b64]);
    vec![m]
}

/// 判断用户是否明确要求以图表形式呈现(流程图、时序图、脑图等)
///
/// 参数: question 用户问题
/// 返回: 明确要图返回 true;其余情况一律只用文字回答
pub fn wants_diagram(question: &str) -> bool {
    let q = question.to_lowercase();
    const KEYS: [&str; 16] = [
        "流程图",
        "时序图",
        "甘特图",
        "脑图",
        "思维导图",
        "架构图",
        "关系图",
        "示意图",
        "图表",
        "画图",
        "画个图",
        "画一个图",
        "画出",
        "可视化",
        "mermaid",
        "flowchart",
    ];
    KEYS.iter().any(|k| q.contains(k))
}

/// 构造问答系统提示词
///
/// 参数: today 今天日期文案;context 已编号的资料文本;allow_diagram 用户是否明确要图
/// 返回: 系统提示词
pub fn chat_system_prompt(today: &str, context: &str, allow_diagram: bool) -> String {
    let diagram_rule = if allow_diagram {
        "3. 用户明确要求图表,请先给出简短文字说明,再附一个 ```mermaid 代码块(flowchart TD、sequenceDiagram、timeline 等)。\n\
            Mermaid 规范:节点 ID 只用英文字母和数字;节点文字必须放在双引号里,如 A[\"文字\"];不要在文字中使用英文括号、引号和分号。\n"
    } else {
        "3. 只用文字回答(可用列表、表格),不要输出 mermaid 代码块或任何图表。\n"
    };
    format!(
        "你是用户的个人知识库助手 MemFlow。今天是 {today}。\n\
         严格遵守以下规则:\n\
         1. 只能依据下方「资料」回答。资料中没有的信息,明确说\"记录中没有提到\",绝不编造。\n\
         2. 引用资料时,在对应句末用方括号标注资料编号,例如 [1] 或 [1][3]。只能使用资料里存在的编号。\n\
         {diagram_rule}\
         4. 使用简体中文,条理清晰,简洁准确。\n\
         \n\
         资料:\n{context}"
    )
}

/// 构造"查询改写"消息:把带指代的追问改写成独立完整的检索问句
///
/// 参数: history 最近对话摘要文本;question 当前问题
/// 返回: 消息列表
pub fn rewrite_messages(history: &str, question: &str) -> Vec<ChatMessage> {
    vec![
        ChatMessage::new(
            "system",
            "你负责把用户的追问改写成一个独立、完整、适合检索的中文问句。补全指代(它/那个/他)。只输出改写后的问句本身,不要解释。",
        ),
        ChatMessage::new("user", format!("对话历史:\n{history}\n\n当前问题:{question}\n\n改写后的问句:")),
    ]
}

/// 构造"修复 Mermaid 语法"消息
///
/// 参数: code 出错的代码;error 报错信息
/// 返回: 消息列表
pub fn mermaid_fix_messages(code: &str, error: &str) -> Vec<ChatMessage> {
    vec![
        ChatMessage::new(
            "system",
            "你是 Mermaid 语法专家。修复用户给出的 Mermaid 代码的语法错误,保持图的含义不变。\
             规则:节点 ID 只用英文字母和数字;节点文字全部放进双引号,如 A[\"文字\"];边上的文字用 |\"文字\"| 形式;不要使用括号、分号等特殊字符作为文字。\
             只输出修复后的 Mermaid 代码,不要代码围栏,不要任何解释。",
        ),
        ChatMessage::new("user", format!("错误信息:{error}\n\n代码:\n{code}")),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 仅明确要图时才允许出图() {
        assert!(wants_diagram("把这个项目的流程画成流程图"));
        assert!(wants_diagram("帮我画个图"));
        assert!(wants_diagram("用 Mermaid 展示"));
        assert!(!wants_diagram("最近我都记录了哪些事?"));
        assert!(!wants_diagram("总结一下项目进展"));
    }

    #[test]
    fn 提示词按需包含图表规则() {
        let on = chat_system_prompt("2026-10-15", "资料", true);
        let off = chat_system_prompt("2026-10-15", "资料", false);
        assert!(on.contains("```mermaid") && on.contains("Mermaid 规范"));
        assert!(off.contains("不要输出 mermaid") && !off.contains("Mermaid 规范"));
        assert!(off.contains("4. 使用简体中文"));
    }
}
