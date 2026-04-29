import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import test from 'node:test';
import vm from 'node:vm';

const source = readFileSync('src/channels/web/static/app.js', 'utf8');
const helperStart = source.indexOf('function selectedSkillBundleFile');
const helperEnd = source.indexOf('/**\n * Install the `.skill` bundle', helperStart);

assert.notEqual(helperStart, -1, 'bundle upload helper functions should exist');
assert.notEqual(helperEnd, -1, 'bundle upload helper block should end before the form handler');

class FakeFormData {
  constructor() {
    this.entries = [];
  }

  append(name, value) {
    this.entries.push([name, value]);
  }
}

const helpers = new vm.Script(`
${source.slice(helperStart, helperEnd)}
({
  selectedSkillBundleFile,
  isSkillBundleFilename,
  buildSkillBundleInstallRequest,
});
`).runInNewContext({ FormData: FakeFormData });

const formHandlerStart = source.indexOf('function installSkillBundleFromForm');
const formHandlerEnd = source.indexOf('\n// Wire up Enter key', formHandlerStart + 1);
const formHandlerSrc = source.slice(
  formHandlerStart,
  formHandlerEnd === -1 ? undefined : formHandlerEnd,
);

test('selectedSkillBundleFile returns the first selected file', () => {
  const first = { name: 'deploy-docs.skill' };
  const second = { name: 'other.skill' };

  assert.equal(helpers.selectedSkillBundleFile({ files: [first, second] }), first);
});

test('selectedSkillBundleFile returns null when no file is selected', () => {
  assert.equal(helpers.selectedSkillBundleFile(null), null);
  assert.equal(helpers.selectedSkillBundleFile({ files: [] }), null);
});

test('isSkillBundleFilename only accepts .skill filenames', () => {
  assert.equal(helpers.isSkillBundleFilename('deploy-docs.skill'), true);
  assert.equal(helpers.isSkillBundleFilename('deploy-docs.zip'), false);
  assert.equal(helpers.isSkillBundleFilename(''), false);
  assert.equal(helpers.isSkillBundleFilename(null), false);
});

test('buildSkillBundleInstallRequest creates a multipart install request', () => {
  const file = { name: 'deploy-docs.skill' };
  const request = helpers.buildSkillBundleInstallRequest(file);

  assert.equal(request.method, 'POST');
  assert.deepEqual(Object.entries(request.headers), [['X-Confirm-Action', 'true']]);
  assert.equal(request.body.entries.length, 1);
  assert.equal(request.body.entries[0][0], 'bundle');
  assert.equal(request.body.entries[0][1], file);
});

for (const [label, fileInput] of [
  ['no file is selected', { files: [] }],
  ['non-.skill filename', { files: [{ name: 'bundle.zip' }], value: '' }],
]) {
  test(`installSkillBundleFromForm shows toast and returns early when ${label}`, () => {
    const calls = { showToast: [] };
    const ctx = {
      document: { getElementById: () => fileInput },
      confirm: () => true,
      apiFetch: async () => ({}),
      showToast: (msg, type) => calls.showToast.push({ msg, type }),
      loadSkills: () => {},
      FormData: FakeFormData,
      selectedSkillBundleFile: helpers.selectedSkillBundleFile,
      isSkillBundleFilename: helpers.isSkillBundleFilename,
      buildSkillBundleInstallRequest: helpers.buildSkillBundleInstallRequest,
    };
    new vm.Script(formHandlerSrc + '\n installSkillBundleFromForm();').runInNewContext(ctx);
    assert.equal(calls.showToast.length, 1);
    assert.equal(calls.showToast[0].type, 'error');
  });
}

test('installSkillBundleFromForm returns early when user cancels confirm', () => {
  let fetchCalled = false;
  const ctx = {
    document: { getElementById: () => ({ files: [{ name: 'deploy-docs.skill' }], value: '' }) },
    confirm: () => false,
    apiFetch: async () => { fetchCalled = true; return {}; },
    showToast: () => {},
    loadSkills: () => {},
    FormData: FakeFormData,
    selectedSkillBundleFile: helpers.selectedSkillBundleFile,
    isSkillBundleFilename: helpers.isSkillBundleFilename,
    buildSkillBundleInstallRequest: helpers.buildSkillBundleInstallRequest,
  };
  new vm.Script(formHandlerSrc + '\n installSkillBundleFromForm();').runInNewContext(ctx);
  assert.equal(fetchCalled, false);
});

test('installSkillBundleFromForm calls apiFetch and shows success toast on install', async () => {
  const calls = { showToast: [], loadSkills: 0 };
  const input = { files: [{ name: 'deploy-docs.skill' }], value: 'initial' };
  const ctx = {
    document: { getElementById: () => input },
    confirm: () => true,
    apiFetch: async (_url, _req) => ({ success: true }),
    showToast: (msg, type) => calls.showToast.push({ msg, type }),
    loadSkills: () => { calls.loadSkills += 1; },
    FormData: FakeFormData,
    selectedSkillBundleFile: helpers.selectedSkillBundleFile,
    isSkillBundleFilename: helpers.isSkillBundleFilename,
    buildSkillBundleInstallRequest: helpers.buildSkillBundleInstallRequest,
  };
  const script = new vm.Script(formHandlerSrc + '\n installSkillBundleFromForm();');
  script.runInNewContext(ctx);
  await new Promise(resolve => setImmediate(resolve));
  assert.equal(calls.showToast.length, 1);
  assert.equal(calls.showToast[0].type, 'success');
  assert.equal(calls.loadSkills, 1);
  assert.equal(input.value, '');
});
