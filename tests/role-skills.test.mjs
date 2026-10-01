import assert from 'node:assert/strict'
import { mkdir, mkdtemp, readFile, rm, stat, writeFile } from 'node:fs/promises'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import test from 'node:test'
import { roleInstructions } from '../src/core/roles.js'
import { roleConfiguration } from '../src/role-skills.js'
import { fakeNodeExecutable } from './helpers.mjs'

async function fixture(t) {
  const root = await mkdtemp(join(tmpdir(), 'cf-role-skills-'))
  t.after(() => rm(root, { recursive: true, force: true }))
  return { root, env: { HOME: root, CONSENSFLOW_HOME: join(root, 'app') } }
}

const worker = { name: 'saved-worker', roles: ['worker'], workTier: 'complex' }

/** Where each harness is told to find the role text it loads. */
function roleFile(kind, configuration) {
  if (kind === 'claude-code')
    return configuration.args[configuration.args.indexOf('--append-system-prompt-file') + 1]
  if (kind === 'pi') return configuration.args[configuration.args.indexOf('--skill') + 1]
  if (kind === 'opencode')
    return JSON.parse(configuration.env.OPENCODE_CONFIG_CONTENT).instructions.at(-1)
  return configuration.env.CF_DEVIN_ROLE_FILE
}

test('a role text is written in a private directory, and global skills are left alone', async (t) => {
  const { root, env } = await fixture(t)
  const global = join(root, '.claude', 'skills', 'consensflow')
  await mkdir(global, { recursive: true })
  await writeFile(join(global, 'SKILL.md'), 'global canary')
  const content = roleInstructions('chief', [worker])
  const configuration = await roleConfiguration('claude-code', {
    role: 'chief',
    env,
    launch: 'launch-1',
    content,
  })
  assert.equal(configuration.args[0], '--add-dir')
  assert.ok(configuration.args[1].startsWith(env.CONSENSFLOW_HOME))
  const text = await readFile(
    join(configuration.args[1], '.claude/skills/consensflow-chief/SKILL.md'),
    'utf8',
  )
  assert.equal(text, content)
  assert.equal(await readFile(join(global, 'SKILL.md'), 'utf8'), 'global canary')
})

test('a window without its role text is refused', async (t) => {
  const { env } = await fixture(t)
  await assert.rejects(
    roleConfiguration('pi', { role: 'worker', env, launch: 'launch-1' }),
    /needs its role text/,
  )
  await assert.rejects(readFile(join(env.CONSENSFLOW_HOME, 'integrations')), { code: 'ENOENT' })
})

test("each launch has a role file of its own: one chief never reads another project's staff", async (t) => {
  const { env } = await fixture(t)
  const folders = { 'claude-code': 'claude', opencode: 'opencode', pi: 'pi', devin: 'devin' }
  for (const [kind, folder] of Object.entries(folders)) {
    const files = []
    for (const [launch, staff] of [
      [`${kind}-a`, [worker]],
      [`${kind}-b`, []],
    ]) {
      const configuration = await roleConfiguration(kind, {
        role: 'chief',
        env,
        launch,
        content: roleInstructions('chief', staff),
      })
      const file = roleFile(kind, configuration)
      assert.ok(file.startsWith(join(env.CONSENSFLOW_HOME, 'integrations', folder, launch)), file)
      files.push(file)
    }
    assert.match(await readFile(files[0], 'utf8'), /\| saved-worker \| worker \| Complex work \|/)
    assert.doesNotMatch(await readFile(files[1], 'utf8'), /saved-worker/, kind)
  }
})

test('every role enters every harness with its whole text already loaded', async (t) => {
  for (const kind of ['claude-code', 'codex', 'opencode', 'pi', 'devin']) {
    for (const role of ['chief', 'advisor', 'worker', 'reviewer', 'designer']) {
      await t.test(`${kind} ${role}`, async (t) => {
        const { env } = await fixture(t)
        const existing = 'User instructions: preserve "quotes", `backticks`, $HOME\nand newlines.'
        const original = {
          theme: 'user',
          skills: { paths: ['/user/skills'], urls: ['https://example.com/skills'] },
          instructions: ['/user/rules.md'],
        }
        env.OPENCODE_CONFIG_CONTENT = JSON.stringify(original)
        const content = roleInstructions(role, [worker])
        const configuration = await roleConfiguration(kind, {
          role,
          env,
          launch: 'launch-1',
          content,
          readInstructions: async () => existing,
        })
        if (kind !== 'codex') {
          const file = roleFile(kind, configuration)
          assert.match(file.replaceAll('\\', '/'), new RegExp(`/consensflow-${role}/SKILL\\.md$`))
          assert.equal(await readFile(file, 'utf8'), content)
        }
        let instructions
        if (kind === 'claude-code') {
          const index = configuration.args.indexOf('--append-system-prompt-file')
          assert.notEqual(index, -1, 'the full role must enter the native system prompt')
          instructions = await readFile(configuration.args[index + 1], 'utf8')
          assert.equal(
            configuration.args[configuration.args.indexOf('--system-prompt-snapshot') + 1],
            'off',
            'resumed conversations must use the current role instructions',
          )
        } else if (kind === 'pi') {
          assert.equal(configuration.args[0], '--skill')
          const index = configuration.args.indexOf('--append-system-prompt')
          assert.notEqual(index, -1, '--skill alone only advertises the role')
          instructions = configuration.args[index + 1]
        } else if (kind === 'opencode') {
          const merged = JSON.parse(configuration.env.OPENCODE_CONFIG_CONTENT)
          assert.deepEqual(merged.instructions, [
            ...original.instructions,
            roleFile(kind, configuration),
          ])
          assert.equal(merged.theme, original.theme)
          assert.deepEqual(merged.skills.urls, original.skills.urls)
          assert.equal(merged.skills.paths[0], original.skills.paths[0])
          instructions = await readFile(merged.instructions.at(-1), 'utf8')
          const repeated = await roleConfiguration(kind, {
            role,
            env: { ...env, ...configuration.env },
            launch: 'launch-1',
            content,
          })
          assert.deepEqual(JSON.parse(repeated.env.OPENCODE_CONFIG_CONTENT), merged)
        } else if (kind === 'devin') {
          instructions = await readFile(configuration.env.CF_DEVIN_ROLE_FILE, 'utf8')
        } else {
          instructions = JSON.parse(configuration.args[1].slice('developer_instructions='.length))
          assert.ok(instructions.startsWith(`${existing}\n\n`))
        }
        assert.ok(instructions.includes(content), 'the whole role text is loaded')
        if (role === 'chief') {
          assert.match(instructions, /\| saved-worker \| worker \| Complex work \|/)
        } else {
          assert.doesNotMatch(instructions, /\| saved-worker \|/)
        }
        assert.equal(env.OPENCODE_CONFIG_CONTENT, JSON.stringify(original))
      })
    }
  }
})

test('OpenCode rejects malformed instruction lists before native launch', async (t) => {
  for (const instructions of ['rules.md', null, {}, 7, ['rules.md', 7]]) {
    await t.test(JSON.stringify(instructions), async (t) => {
      const { env } = await fixture(t)
      env.OPENCODE_CONFIG_CONTENT = JSON.stringify({ instructions })
      await assert.rejects(
        roleConfiguration('opencode', {
          role: 'chief',
          env,
          launch: 'launch-1',
          content: roleInstructions('chief', []),
        }),
        /OpenCode instructions must be an array of paths/,
      )
    })
  }
})

test('Codex appends the role to its own effective instructions, read through configuration only', async (t) => {
  const { root, env } = await fixture(t)
  const executable = fakeNodeExecutable(
    join(root, 'codex'),
    `#!${process.execPath}\n
import { createInterface } from 'node:readline';
const lines = createInterface({input: process.stdin});
lines.on('line', line => {
  const request = JSON.parse(line);
  if (request.method === 'initialize') console.log(JSON.stringify({id:request.id,result:{}}));
  else if (request.method === 'config/read') console.log(JSON.stringify({id:request.id,result:{config:{developer_instructions:'existing user instructions'}}}));
  else if (request.method !== 'initialized') process.exit(20);
});
`,
  )
  const configuration = await roleConfiguration('codex', {
    role: 'chief',
    env,
    executable,
    cwd: root,
    content: roleInstructions('chief', []),
  })
  assert.equal(configuration.args[0], '-c')
  assert.match(configuration.args[1], /existing user instructions/)
  assert.match(configuration.args[1], /consensflow-chief/)
  assert.doesNotMatch(configuration.args[1], /consensflow-worker/)
})

test('a role text is private to its launch, and a launch id names nothing outside it', async (t) => {
  const { env } = await fixture(t)
  const configured = await roleConfiguration('pi', {
    role: 'worker',
    env,
    launch: 'launch-1',
    content: 'worker text',
  })
  const file = configured.args[1]
  assert.equal(await readFile(file, 'utf8'), 'worker text')
  assert.equal(configured.args.at(-1), 'worker text')
  // Windows has no POSIX modes; its files answer 0o666 whatever the writer asked.
  if (process.platform !== 'win32') assert.equal((await stat(file)).mode & 0o777, 0o600)
  for (const launch of ['..', '../elsewhere', undefined])
    await assert.rejects(
      roleConfiguration('pi', { role: 'worker', env, launch, content: 'worker text' }),
      /launch/,
    )
})
