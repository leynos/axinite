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
