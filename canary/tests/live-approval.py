#!/usr/bin/env python3
"""Live matrices require one successful review; skipped reviews never authorize calls."""
import ast
from pathlib import Path
import re
import unittest

ROOT = Path(__file__).resolve().parents[2]
WORKFLOWS = ROOT / '.github/workflows'
CONSUMERS = {
    'release-gate.yml': ['cell', 'opencode-v2'],
    'cli-release-gate.yml': ['cli'],
    'desktop-check.yml': ['check'],
    'desktop-check-suite.yml': ['desktop'],
    'windows-cli-diagnostic.yml': ['diagnostic', 'omp-usage-comparison'],
}


def jobs(path):
    body = path.read_text().split('\njobs:\n', 1)[1]
    return dict(re.findall(r'^  ([a-z][a-z0-9-]*):\n(.*?)(?=^  [a-z][a-z0-9-]*:\n|\Z)', body, re.M | re.S))


def condition(job, *, mode='live', live=True, comparison=False, approval='success',
              cancelled=False, prerequisite='success', input_validation='success',
              has_cells='true', ref='refs/heads/main'):
    expression = re.search(r'^    if: (.+)$', job, re.M).group(1)
    expression = expression.removeprefix('${{').removesuffix('}}').strip()
    values = {'inputs.mode': mode, 'inputs.live': live, 'inputs.omp_usage_comparison': comparison,
              'needs.approve-live.result': approval, 'needs.prepare.result': prerequisite,
              'needs.select.result': prerequisite, 'needs.select.outputs.has_cells': has_cells,
              'needs.validate-inputs.result': input_validation,
              'github.ref': ref}
    for key in sorted(values, key=len, reverse=True):
        expression = re.sub(r'(?<![\w.-])' + re.escape(key) + r'(?![\w.-])', repr(values[key]), expression)
    expression = re.sub(r'!(?!=)', ' not ', expression).replace('&&', ' and ').replace('||', ' or ').strip()
    tree = ast.parse(expression, mode='eval')
    allowed = (ast.Expression, ast.BoolOp, ast.And, ast.Or, ast.UnaryOp, ast.Not, ast.Compare,
               ast.Eq, ast.NotEq, ast.Constant, ast.Call, ast.Name, ast.Load)
    for node in ast.walk(tree):
        if not isinstance(node, allowed):
            raise ValueError('unexpected workflow condition')
        if isinstance(node, ast.Name) and node.id not in {'always', 'cancelled'}:
            raise ValueError('unknown workflow status function')
        if isinstance(node, ast.Call) and (not isinstance(node.func, ast.Name) or node.args or node.keywords):
            raise ValueError('unexpected workflow status call')
    return eval(compile(tree, '<workflow-condition>', 'eval'), {'__builtins__': {}},
                {'always': lambda: True, 'cancelled': lambda: cancelled})


class LiveApproval(unittest.TestCase):
    def test_inventory_observation_routes_only_to_the_credential_free_reusable_job(self):
        workflow = jobs(WORKFLOWS / 'windows-cli-diagnostic.yml')
        observer = workflow['mimo-inventory-observation']
        self.assertIn('needs: validate-inputs', observer)
        self.assertIn('uses: ./.github/workflows/mimo-inventory-diagnostic.yml', observer)
        self.assertNotIn('environment:', observer)
        self.assertNotIn('secrets', observer)
        mode = 'mimo-inventory-observation'
        self.assertTrue(condition(observer, mode=mode, approval='skipped'))
        self.assertTrue(condition(observer, mode=mode, ref='refs/heads/diagnostic', approval='skipped'))
        self.assertFalse(condition(observer, mode=mode, cancelled=True))
        for validation in ('failure', 'cancelled', 'skipped', 'timed_out'):
            self.assertFalse(condition(observer, mode=mode, input_validation=validation))
        for other_mode in ('live', 'deterministic', 'native-diagnostic', 'codex-diagnostic'):
            self.assertFalse(condition(observer, mode=other_mode))
        for name in ('approve-live', 'diagnostic', 'omp-usage-comparison'):
            for comparison in (False, True):
                with self.subTest(job=name, comparison=comparison):
                    self.assertFalse(condition(workflow[name], mode=mode, comparison=comparison,
                                               approval='success'))

    def test_failed_windows_input_validation_blocks_review_and_credentials_even_with_approval(self):
        workflow = jobs(WORKFLOWS / 'windows-cli-diagnostic.yml')
        validator = workflow['validate-inputs']
        self.assertIn('permissions: {}', validator)
        self.assertNotIn('environment:', validator)
        self.assertNotIn('secrets.', validator)
        for name in ('approve-live', 'diagnostic', 'omp-usage-comparison'):
            job = workflow[name]
            self.assertIsNotNone(re.search(r'^    needs: (?:validate-inputs|\[[^\n]*validate-inputs[^\n]*\])$', job, re.M))
            comparison = name == 'omp-usage-comparison'
            self.assertTrue(condition(job, comparison=comparison))
            for validation in ('failure', 'cancelled', 'skipped', 'timed_out'):
                for mode in ('live', 'deterministic'):
                    with self.subTest(job=name, validation=validation, mode=mode):
                        self.assertFalse(condition(job, mode=mode, comparison=comparison,
                                                   input_validation=validation, approval='success'))
        # Deterministic diagnostics still need valid inputs when there is no review at all.
        self.assertTrue(condition(workflow['diagnostic'], mode='deterministic', approval='skipped'))
        self.assertFalse(condition(workflow['diagnostic'], mode='deterministic', approval='skipped',
                                   input_validation='failure'))

    def test_every_live_worker_has_a_direct_fail_closed_approval_dependency(self):
        observed = set()
        for filename, names in CONSUMERS.items():
            workflow = jobs(WORKFLOWS / filename)
            guard = workflow['approve-live']
            self.assertIn('uses: ./.github/workflows/live-check-approval.yml', guard)
            self.assertNotIn('matrix:', guard)
            self.assertNotIn('secrets.', guard)
            for name, job in workflow.items():
                if 'canary-live-workers' in job:
                    observed.add((filename, name))
                    self.assertIn(name, names)
                    self.assertIsNotNone(re.search(r'^    needs: (?:approve-live|\[[^\n]*approve-live[^\n]*\])$', job, re.M))
                    self.assertIn('deployment: false', job)
            for name in names:
                job = workflow[name]
                comparison = name == 'omp-usage-comparison'
                self.assertTrue(condition(job, comparison=comparison))
                for approval in ('failure', 'cancelled', 'skipped', 'timed_out'):
                    self.assertFalse(condition(job, comparison=comparison, approval=approval), (filename, name, approval))
                self.assertFalse(condition(job, comparison=comparison, cancelled=True))
                self.assertFalse(condition(job, comparison=comparison, ref='refs/heads/untrusted'))
                if 'needs.select.result' in job or 'needs.prepare.result' in job:
                    self.assertFalse(condition(job, comparison=comparison, prerequisite='failure'))
                if not comparison:
                    self.assertTrue(condition(job, mode='deterministic', live=False, approval='skipped'))
        self.assertEqual(observed, {(filename, name) for filename, names in CONSUMERS.items() for name in names})

    def test_comparison_is_live_even_with_deterministic_mode_and_empty_cli_selection_is_skipped(self):
        workflow = jobs(WORKFLOWS / 'windows-cli-diagnostic.yml')
        self.assertTrue(condition(workflow['approve-live'], mode='deterministic', comparison=True))
        self.assertTrue(condition(workflow['omp-usage-comparison'], mode='deterministic', comparison=True))
        self.assertFalse(condition(workflow['omp-usage-comparison'], mode='deterministic', comparison=True, approval='skipped'))
        cli = jobs(WORKFLOWS / 'cli-release-gate.yml')
        self.assertFalse(condition(cli['cli'], has_cells='false'))
        self.assertFalse(condition(cli['approve-live'], has_cells='false'))

    def test_single_reviewer_job_has_no_checkout_or_credential_materialization(self):
        workflow = jobs(WORKFLOWS / 'live-check-approval.yml')
        self.assertEqual(set(workflow), {'approve'})
        guard = workflow['approve']
        self.assertIn('name: canary-live', guard)
        self.assertNotIn('matrix:', guard)
        self.assertNotIn('checkout', guard)
        self.assertNotIn('secrets.', guard)
        self.assertNotIn('NAN_API_KEY', guard)
        self.assertIn('deployment: false', guard)
        desktop = jobs(WORKFLOWS / 'desktop-check.yml')
        self.assertFalse(condition(desktop['check'], ref='refs/heads/untrusted'))

    def test_release_desktop_qualification_waits_for_the_same_single_review(self):
        release = jobs(WORKFLOWS / 'release-gate.yml')
        desktop = release['desktop']
        self.assertIn('needs: [prepare, approve-live]', desktop)
        self.assertIn('12 native Desktop cells', release['approve-live'])
        self.assertTrue(condition(desktop))
        for approval in ('failure', 'cancelled', 'skipped'):
            self.assertFalse(condition(desktop, approval=approval))
        self.assertFalse(condition(desktop, prerequisite='failure'))
        self.assertFalse(condition(desktop, cancelled=True))
        self.assertFalse(condition(desktop, ref='refs/heads/untrusted'))
        self.assertTrue(condition(desktop, mode='deterministic', approval='skipped'))


if __name__ == '__main__':
    unittest.main()
