import assert from 'node:assert/strict'
import { chmod, mkdir, mkdtemp, readFile, rm, stat, utimes, writeFile } from 'node:fs/promises'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import test from 'node:test'
import { roleInstructions } from '../src/core/roles.js'
import { roleConfiguration } from '../src/role-skills.js'

async function fixture(t) {
  const root = await mkdtemp(join(tmpdir(), 'cf-role-skills-'))
  t.after(() => rm(root, { recursive: true, force: true }))
  return { root, env: { HOME: root, CONSENSFLOW_HOME: join(root, 'app') } }
}

const worker = {
  name: 'saved-worker',
  harness: 'codex',
  model: 'gpt-6-astra',
  effort: 'xhigh',
  tags: ['coding', 'review'],
}

test('a role text is written in a private directory, and global skills are left alone', async (t) => {
  const { root, env } = await fixture(t)
  const global = join(root, '.claude', 'skills', 'consensflow')
  await mkdir(global, { recursive: true })
  await writeFile(join(global, 'SKILL.md'), 'global canary')
  const content = roleInstructions('lead', [worker])
  const configuration = await roleConfiguration('claude-code', { role: 'lead', env, content })
  assert.equal(configuration.args[0], '--add-dir')
  assert.ok(configuration.args[1].startsWith(env.CONSENSFLOW_HOME))
  const text = await readFile(
    join(configuration.args[1], '.claude/skills/consensflow-lead/SKILL.md'),
    'utf8',
  )
  assert.equal(text, content)
  assert.equal(await readFile(join(global, 'SKILL.md'), 'utf8'), 'global canary')
})

test('a window without its role text is refused', async (t) => {
  const { env } = await fixture(t)
  await assert.rejects(roleConfiguration('pi', { role: 'worker', env }), /needs its role text/)
  await assert.rejects(readFile(join(env.CONSENSFLOW_HOME, 'roles')), { code: 'ENOENT' })
})

test('every role enters every harness with its whole text already loaded', async (t) => {
  for (const kind of ['claude-code', 'codex', 'opencode', 'pi', 'devin']) {
    for (const role of ['lead', 'pm', 'advisor', 'worker', 'reviewer']) {
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
          content,
          readInstructions: async () => existing,
        })
        const file = join(
          env.CONSENSFLOW_HOME,
          'roles',
          role,
          '.claude',
          'skills',
          `consensflow-${role}`,
          'SKILL.md',
        )
        assert.equal(await readFile(file, 'utf8'), content)
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
          assert.deepEqual(merged.instructions, [...original.instructions, file])
          assert.equal(merged.theme, original.theme)
          assert.deepEqual(merged.skills.urls, original.skills.urls)
          assert.equal(merged.skills.paths[0], original.skills.paths[0])
          instructions = await readFile(merged.instructions.at(-1), 'utf8')
          const repeated = await roleConfiguration(kind, {
            role,
            env: { ...env, ...configuration.env },
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
        if (role === 'lead' || role === 'pm') {
          assert.match(instructions, /\| saved-worker \| [^|]+ \| coding, review \|/)
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
        roleConfiguration('opencode', { role: 'pm', env, content: roleInstructions('pm', []) }),
        /OpenCode instructions must be an array of paths/,
      )
    })
  }
})

test('Codex appends the role to its own effective instructions, read through configuration only', async (t) => {
  const { root, env } = await fixture(t)
  const executable = join(root, 'codex')
  await writeFile(
    executable,
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
  await chmod(executable, 0o755)
  const configuration = await roleConfiguration('codex', {
    role: 'lead',
    env,
    executable,
    cwd: root,
    content: roleInstructions('lead', []),
  })
  assert.equal(configuration.args[0], '-c')
  assert.match(configuration.args[1], /existing user instructions/)
  assert.match(configuration.args[1], /consensflow-lead/)
  assert.doesNotMatch(configuration.args[1], /consensflow-pm/)
})

test('an unchanged role text is not rewritten, a changed one is, always private', async (t) => {
  const { env } = await fixture(t)
  const first = await roleConfiguration('pi', { role: 'worker', env, content: 'worker text, v1' })
  const file = first.args[1]
  await utimes(file, 1, 1)
  const timestamp = (await stat(file)).mtimeMs
  assert.deepEqual(
    await roleConfiguration('pi', { role: 'worker', env, content: 'worker text, v1' }),
    first,
  )
  assert.equal((await stat(file)).mtimeMs, timestamp, 'unchanged role is not rewritten')
  const refreshed = await roleConfiguration('pi', {
    role: 'worker',
    env,
    content: 'worker text, v2',
  })
  assert.equal(await readFile(file, 'utf8'), 'worker text, v2')
  assert.equal(refreshed.args.at(-1), 'worker text, v2')
  assert.equal((await stat(file)).mode & 0o777, 0o600)
})
