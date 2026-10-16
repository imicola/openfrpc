<script setup>
import { reactive, ref, watch, computed } from "vue";
import Modal from "./Modal.vue";
import Toggle from "./Toggle.vue";

const props = defineProps({
  modelValue: Boolean, // 打开
  entry: Object, // 编辑时传入已有条目（null = 新建）
  renameLocked: { type: Boolean, default: false },
});
const emit = defineEmits(["update:modelValue", "submit"]);

const TYPES = [
  { v: "tcp", label: "TCP", desc: "把本地端口映射到服务器端口" },
  { v: "udp", label: "UDP", desc: "转发 UDP 流量（如游戏、DNS）" },
  { v: "http", label: "HTTP", desc: "网站服务，支持子域名/域名" },
  { v: "https", label: "HTTPS", desc: "HTTPS 网站服务" },
  { v: "stcp", label: "STCP", desc: "私密 TCP，需密钥访问" },
  { v: "xtcp", label: "XTCP", desc: "点对点穿透，失败回退 STCP" },
  { v: "sudp", label: "SUDP", desc: "私密 UDP，需密钥访问" },
];

function blank() {
  return {
    name: "", ptype: "tcp", local_ip: "127.0.0.1", local_port: null,
    remote_port: null, subdomain: "", customDomainsText: "", locationsText: "",
    secret_key: "", http_user: "", http_pwd: "", use_encryption: false, use_compression: false,
  };
}

const form = reactive(blank());
const err = ref("");

// R19：插件后端的本地地址/端口由插件决定，界面不要求填写也不改动
const isPlugin = computed(() => !!props.entry?.plugin);

watch(() => props.modelValue, (open) => {
  if (open) {
    err.value = "";
    const e = props.entry;
    Object.assign(form, blank());
    if (e) {
      form.name = e.name;
      form.ptype = e.ptype;
      form.local_ip = e.local_ip || "127.0.0.1";
      form.local_port = e.local_port;
      form.remote_port = e.remote_port ?? null;
      form.subdomain = e.subdomain || "";
      form.customDomainsText = (e.custom_domains || []).join(", ");
      form.locationsText = (e.locations || []).join(", ");
      form.secret_key = e.secret_key || "";
      form.http_user = e.http_user || "";
      form.http_pwd = e.http_pwd || "";
      form.use_encryption = !!e.use_encryption;
      form.use_compression = !!e.use_compression;
    }
  }
});

const curType = computed(() => TYPES.find((t) => t.v === form.ptype));
const needRemote = computed(() => form.ptype === "tcp" || form.ptype === "udp");
const isWeb = computed(() => form.ptype === "http" || form.ptype === "https");
const isSecret = computed(() => ["stcp", "xtcp", "sudp"].includes(form.ptype));

const splitList = (s) => s.split(/[,，\s]+/).map((x) => x.trim()).filter(Boolean);

function submit() {
  if (!form.name.trim()) { err.value = "请填写代理名称"; return; }
  if (!/^[a-zA-Z0-9_-]{1,60}$/.test(form.name.trim())) { err.value = "名称仅限字母、数字、下划线和短横线"; return; }
  // R19：插件条目的本地端口由插件提供，不做 1–65535 校验
  if (!isPlugin.value && (!form.local_port || form.local_port < 1 || form.local_port > 65535)) {
    err.value = "本地端口需在 1–65535 之间"; return;
  }
  if (needRemote.value && (!form.remote_port || form.remote_port < 1 || form.remote_port > 65535)) {
    err.value = "远端端口需在 1–65535 之间";
    return;
  }
  if (isSecret.value && !form.secret_key.trim()) { err.value = "私密类型需要设置访问密钥（secretKey）"; return; }

  emit("submit", {
    name: form.name.trim(),
    ptype: form.ptype,
    local_ip: isPlugin.value ? (props.entry.local_ip || "127.0.0.1") : (form.local_ip || "127.0.0.1"),
    local_port: isPlugin.value ? (props.entry.local_port ?? 0) : Number(form.local_port),
    remote_port: needRemote.value ? Number(form.remote_port) : null,
    // R14：字段按类型显式清空，切换类型不残留旧类型的专属字段
    subdomain: isWeb.value ? (form.subdomain.trim() || null) : null,
    custom_domains: isWeb.value && form.customDomainsText.trim() ? splitList(form.customDomainsText) : null,
    locations: form.ptype === "http" && form.locationsText.trim() ? splitList(form.locationsText) : null,
    secret_key: isSecret.value ? form.secret_key.trim() : null,
    // R13：frp 0.71 的 HTTP 认证是 httpUser/httpPassword（HTTPS 类型无此字段）
    http_user: form.ptype === "http" && form.http_user.trim() ? form.http_user.trim() : null,
    http_pwd: form.ptype === "http" && form.http_pwd.trim() ? form.http_pwd.trim() : null,
    use_encryption: form.use_encryption || null,
    use_compression: form.use_compression || null,
    plugin: isPlugin.value ? props.entry.plugin : null,
  });
}
</script>

<template>
  <Modal :title="entry ? '编辑代理' : '新建代理'" :model-value="modelValue" wide @close="emit('update:modelValue', false)">
    <div class="col" style="gap:16px">
      <div class="field">
        <label>代理类型</label>
        <div class="type-grid">
          <button v-for="t in TYPES" :key="t.v" class="type-card" :class="{ on: form.ptype === t.v }"
            @click="form.ptype = t.v" type="button">
            <b>{{ t.label }}</b>
            <span>{{ t.desc }}</span>
          </button>
        </div>
      </div>

      <div class="form-grid">
        <div class="field">
          <label>名称 <span class="desc">服务器端需要唯一</span></label>
          <input class="input mono" v-model="form.name" placeholder="如 dev-web" :disabled="renameLocked && !!entry" />
        </div>
        <template v-if="isPlugin">
          <div class="field" style="grid-column: span 3">
            <label>本地服务 <span class="desc">插件条目</span></label>
            <div class="muted small" style="padding:7px 0">
              该代理使用插件「{{ entry.plugin }}」，本地地址与端口由插件配置决定，此处不修改。
            </div>
          </div>
        </template>
        <template v-else>
          <div class="field">
            <label>本地地址</label>
            <input class="input mono" v-model="form.local_ip" placeholder="127.0.0.1" />
          </div>
          <div class="field">
            <label>本地端口</label>
            <input class="input mono" v-model.number="form.local_port" type="number" min="1" max="65535" placeholder="8770" />
          </div>
        </template>
        <div class="field" v-if="needRemote">
          <label>远端端口 <span class="desc">服务器上暴露的端口</span></label>
          <input class="input mono" v-model.number="form.remote_port" type="number" min="1" max="65535" placeholder="6022" />
        </div>
        <div class="field" v-if="isWeb">
          <label>子域名 <span class="desc">拼接 frps 的 subdomainHost</span></label>
          <input class="input" v-model="form.subdomain" placeholder="dev" />
        </div>
        <div class="field" v-if="isSecret">
          <label>访问密钥 <span class="desc">访问方需携带相同 secretKey</span></label>
          <input class="input mono" v-model="form.secret_key" placeholder="与访问端约定" />
        </div>
      </div>

      <div class="form-grid" v-if="isWeb">
        <div class="field" style="grid-column: span 2">
          <label>自定义域名 <span class="desc">多个用逗号分隔</span></label>
          <input class="input" v-model="form.customDomainsText" placeholder="a.example.com, b.example.com" />
        </div>
        <div class="field" v-if="form.ptype === 'http'" style="grid-column: span 2">
          <label>路由路径 <span class="desc">locations，仅 HTTP 代理适用</span></label>
          <input class="input mono" v-model="form.locationsText" placeholder="/api, /static" />
        </div>
      </div>

      <div class="form-grid" v-if="form.ptype === 'http'">
        <div class="field">
          <label>HTTP 认证用户名 <span class="desc">httpUser</span></label>
          <input class="input mono" v-model="form.http_user" placeholder="admin" autocomplete="off" />
        </div>
        <div class="field">
          <label>HTTP 认证密码 <span class="desc">httpPassword</span></label>
          <input class="input mono" v-model="form.http_pwd" placeholder="与 frps 端约定" autocomplete="new-password" />
        </div>
      </div>

      <div class="row" style="gap:26px">
        <label class="row" style="gap:8px;cursor:pointer">
          <Toggle :on="form.use_encryption" @toggle="form.use_encryption = !form.use_encryption" />
          <span style="font-size:13px">内容加密 <span class="muted small">useEncryption</span></span>
        </label>
        <label class="row" style="gap:8px;cursor:pointer">
          <Toggle :on="form.use_compression" @toggle="form.use_compression = !form.use_compression" />
          <span style="font-size:13px">内容压缩 <span class="muted small">useCompression</span></span>
        </label>
      </div>

      <div v-if="err" class="badge err" style="align-self:flex-start">{{ err }}</div>
      <div class="muted small">{{ curType?.desc }}。保存时会自动运行 frpc verify 校验，改动即刻写入 frpc.toml。</div>
    </div>

    <template #foot>
      <button class="btn" @click="emit('update:modelValue', false)">取消</button>
      <button class="btn btn-primary" @click="submit">{{ entry ? "保存修改" : "创建代理" }}</button>
    </template>
  </Modal>
</template>

<style scoped>
.type-grid { display: grid; grid-template-columns: repeat(auto-fill, minmax(140px, 1fr)); gap: 8px; }
.type-card {
  border: 1px solid var(--line-2); border-radius: 8px; background: var(--surface);
  padding: 9px 11px; text-align: left; cursor: pointer; font-family: inherit;
  transition: all 0.15s; display: flex; flex-direction: column; gap: 1px;
}
.type-card b { font-size: 13px; color: var(--ink); }
.type-card span { font-size: 11.5px; color: var(--ink-3); line-height: 1.4; }
.type-card:hover { border-color: #bfc9d4; }
.type-card.on { border-color: var(--accent); background: var(--accent-soft); }
.type-card.on b { color: var(--accent); }
</style>
