// The Setup window's status and error texts (app/ui/status.ts).
import assert from 'node:assert/strict';
import test from 'node:test';
import {describeError, describeStatus, isChinese, text} from '../app/ui/status.ts';

const base = {
  version: '1.0.0', installed: true, running: true, reachable: true,
  certificate: {exists: true, trusted: true},
  hosts: [{id: 'word', app: true, ready: true, sideloaded: true}, {id: 'powerpoint', app: false, ready: false, sideloaded: false}],
  model: {source: null, downloadBytes: 108795631}, location: null, url: 'https://localhost:28766',
};

test('describes a working installation', () => {
  const rows = describeStatus('zh', base);
  assert.deepEqual(rows.map(r => [r.label, r.level]), [
    ['插件服务', 'ok'], ['本地证书', 'ok'], ['Word', 'ok'], ['PowerPoint', 'off'], ['截图识别模型', 'warn'],
  ]);
  assert.equal(rows[3].value, '未安装 PowerPoint');
  assert.equal(rows[4].value, '未下载（109 MB）');
  assert.equal(rows[4].action, 'download');
});

test('flags a stopped service, an untrusted certificate and unopened apps', () => {
  const rows = describeStatus('en', {
    ...base, reachable: false, certificate: {exists: true, trusted: false},
    hosts: [{id: 'word', app: true, ready: false, sideloaded: false}, {id: 'powerpoint', app: true, ready: true, sideloaded: false}],
    model: {source: 'desktop', downloadBytes: 1},
  });
  assert.deepEqual(rows.map(r => r.level), ['bad', 'bad', 'warn', 'warn', 'ok']);
  assert.equal(rows[2].value, 'Open Word once, then install');
  assert.equal(rows[4].value, 'Using the model downloaded by LaTeXSnipper');
  const fresh = describeStatus('zh', {...base, installed: false, certificate: {exists: false, trusted: false}});
  assert.deepEqual(fresh.slice(0, 3).map(r => r.level), ['off', 'off', 'off']);
  assert.equal(fresh[2].value, '安装后可用');
});

test('maps host error codes to messages', () => {
  assert.match(describeError('zh', 'TRUST_DENIED:security 失败：User canceled'), /没有信任本地证书/);
  assert.match(describeError('zh', 'LOCATION_DMG:/Volumes/x'), /拖到“应用程序”/);
  assert.match(describeError('en', 'PORT_IN_USE:无法监听 127.0.0.1:28766'), /Port 28766/);
  assert.equal(describeError('en', 'AGENT_FAILED:Bootstrap failed: 5'), 'Could not register the login item: Bootstrap failed: 5');
  assert.equal(describeError('zh', 'DOWNLOAD_FAILED:timed out'), '下载失败：timed out');
  assert.equal(describeError('en', 'Error: boom'), 'Failed: boom');
  assert.equal(text('en', 'downloading', {done: '3', total: '109'}), 'Downloading the formula model: 3 / 109 MB');
  assert.ok(isChinese('zh-Hans-CN') && !isChinese('en-US') && !isChinese(undefined));
});
