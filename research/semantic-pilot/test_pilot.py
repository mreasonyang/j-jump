import copy
import json
import os
from pathlib import Path
import tempfile
import types
import unittest
from unittest.mock import patch
import pilot


class PilotTests(unittest.TestCase):
    def setUp(self):
        self.cases, self.manifest = pilot.load_cases()
        self.case = self.cases[0]
        self.req = pilot.request_for(self.case)
        self.first, self.second = [c['id'] for c in self.case['candidates'][:2]]
        self.manifest = pilot.run_manifest(self.cases, False, 48, 300)

    def record(self, case, selected='none', status='ok', order='original', manifest=None):
        manifest = manifest or self.manifest
        req = pilot.request_for(case, order == 'reversed')
        r = {**pilot.source_identity(), 'run_id': manifest['run_id'],
             'run_manifest_sha256': pilot.digest(pilot.encoded(manifest)),
             'case_id': case['id'], 'order': order,
             'request_sha256': pilot.digest(pilot.encoded(req)), 'status': status, 'elapsed_ms': 12}
        if status == 'ok':
            r['decision'] = {'raw_choice': selected, 'selected': selected, 'confidence': 0.9,
                             'probabilities': {k: 1.0 if k == selected else 0.0 for k in req['questions']['destination']['criteria']},
                             'nouls': {c['id']: 0.5 for c in case['candidates']}}
        elif status == 'http_error':
            r['http_status'] = 429
        return r

    def response(self, choice='none'):
        opts = self.req['questions']['destination']['criteria']
        return {'model': pilot.MODEL, 'answers': {
            'destination': {'type': 'choice', 'choice': choice,
                            'probabilities': {k: 1.0 if k == choice else 0.0 for k in opts}, 'confidence': 0.9},
            **{key: {'type': 'noul', 'noul': 0.5} for key in self.req['questions'] if key != 'destination'}}}

    def test_frozen_counts_and_translation_split(self):
        self.assertEqual(len(self.cases), 48)
        self.assertEqual(sum(c['split'] == 'heldout' for c in self.cases), 32)
        for family in {c['family'] for c in self.cases}:
            members = [c for c in self.cases if c['family'] == family]
            self.assertEqual({c['language'] for c in members}, {'zh', 'en'})
            self.assertEqual(len({c['split'] for c in members}), 1)

    def test_labels_cannot_change_payload(self):
        for c in self.cases:
            other = copy.deepcopy(c)
            other['expected'] = {'secret': 'wrong label and hidden path'}
            other['id'] = 'do not send this'
            other['split'] = 'changed'
            self.assertEqual(pilot.request_for(c), pilot.request_for(other))
            self.assertNotIn('expected', pilot.request_for(c)['state'])

    def test_payload_budget_and_permutation(self):
        for c in self.cases:
            self.assertLess(len(pilot.encoded(pilot.request_for(c))), 65536)
            normal = pilot.request_for(c)['state']['candidates']
            reverse = pilot.request_for(c, True)['state']['candidates']
            self.assertEqual(normal, list(reversed(reverse)))

    def test_strict_json_rejects_duplicates_and_nan(self):
        for raw in ('{"x":1,"x":2}', '{"x":NaN}', '{"x":Infinity}'):
            with self.assertRaises(ValueError): pilot.strict_json(raw)

    def test_unknown_ids_and_question_sets_fail(self):
        for mutate in (lambda r:r.update(model='wrong'),
                       lambda r:r['answers'].pop('match_' + self.first),
                       lambda r:r['answers']['destination'].update(choice='../../evil'),
                       lambda r:r['answers']['destination']['probabilities'].update(extra=0)):
            r = self.response();mutate(r)
            with self.assertRaises(ValueError): pilot.validate_response(r, self.req)

    def test_invalid_probabilities_and_wrong_top_fail(self):
        for val in (True, -0.1, 1.1, float('nan'), float('inf')):
            r = self.response();r['answers']['destination']['confidence'] = val
            with self.assertRaises(ValueError): pilot.validate_response(r, self.req)
        r = self.response();r['answers']['destination']['choice'] = self.first
        with self.assertRaises(ValueError): pilot.validate_response(r, self.req)

    def test_wrong_choice_type_fails_closed(self):
        r = self.response()
        r['answers']['destination']['choice'] = ['c01']
        with self.assertRaises(ValueError): pilot.validate_response(r,self.req)

    def test_all_abstain_does_not_pass_positive_cases(self):
        records = []
        for c in self.cases:
            records.append(self.record(c))
        report = pilot.score(self.cases,records,self.manifest)
        self.assertEqual(report['metrics']['all']['unique_cases'],24)
        self.assertEqual(report['metrics']['all']['unique_top1_correct'],0)
        self.assertEqual(report['metrics']['all']['correct_abstentions'],24)
        self.assertEqual(report['metrics']['all']['unique_abstained'],24)
        self.assertIsNone(report['metrics']['all']['recommendation_precision'])
        self.assertFalse(report['automatic_quality_approved'])

    def test_ties_abstain(self):
        r = self.response(self.first)
        r['answers']['destination']['probabilities'].update({self.first:0.5, self.second:0.5})
        self.assertEqual(pilot.validate_response(r, self.req)['selected'], 'none')

    def test_errors_stay_in_quality_denominators(self):
        record = self.record(self.case, status='http_error')
        report = pilot.score(self.cases, [record], self.manifest)
        self.assertEqual(report['metrics']['all']['invalid_or_error'], 1)
        self.assertEqual(report['metrics']['all']['unique_top1_correct'], 0)
        self.assertEqual(len(report['missing_original_ids']),47)
        self.assertFalse(report['automatic_quality_approved'])

    def test_unknown_duplicate_or_wrong_hash_rejected(self):
        r = self.record(self.case, status='http_error')
        with self.assertRaises(ValueError): pilot.score(self.cases,[r,r], self.manifest)
        r['request_sha256'] = 'bad'
        with self.assertRaises(ValueError): pilot.score(self.cases,[r], self.manifest)

    def test_unique_wrong_choice_counts_as_false_recommendation(self):
        c = next(c for c in self.cases if c['expected']['label'] == 'unique')
        wrong = next(x['id'] for x in c['candidates'] if x['id'] not in c['expected']['acceptable_ids'])
        m = pilot.score(self.cases, [self.record(c, wrong)], self.manifest)['metrics']['all']
        self.assertEqual(m['unique_wrong_target'], 1)
        self.assertEqual(m['false_recommendations'], 1)
        self.assertEqual(m['false_recommendations_on_abstain_cases'], 0)
        self.assertEqual(m['recommendation_precision'], 0)

    def test_all_error_categories_have_distinct_numerators(self):
        unique = [c for c in self.cases if c['expected']['label'] == 'unique']
        negative = [c for c in self.cases if c['expected']['label'] in ('none', 'ambiguous')]
        records = [self.record(unique[0], unique[0]['expected']['acceptable_ids'][0]),
                   self.record(unique[1]), self.record(unique[2], status='http_error'),
                   self.record(negative[0], negative[0]['candidates'][0]['id']),
                   self.record(negative[1])]
        m = pilot.score(self.cases, records, self.manifest)['metrics']['all']
        self.assertEqual((m['attempts'], m['invalid_or_error'], m['unique_abstained'], m['correct_abstentions']), (5,1,1,1))
        self.assertEqual((m['recommendations'], m['correct_recommendations'], m['false_recommendations']), (2,1,1))
        self.assertEqual(m['recommendation_precision'], 0.5)

    def test_every_abstention_label_counts_wrong_advice(self):
        for label in ('none', 'ambiguous', 'insufficient', 'retrieval_miss'):
            c = next(c for c in self.cases if c['expected']['label'] == label)
            m = pilot.score(self.cases, [self.record(c, c['candidates'][0]['id'])], self.manifest)['metrics']['all']
            with self.subTest(label=label):
                self.assertEqual(m['false_recommendations'],1)
                self.assertEqual(m['false_recommendations_on_abstain_cases'],1)
                self.assertEqual(m['unique_wrong_target'],0)

    def test_mismatched_and_missing_result_source_fields_rejected(self):
        for field in (*pilot.source_identity(), 'run_id', 'run_manifest_sha256'):
            for missing in (False, True):
                r = self.record(self.case)
                if missing: r.pop(field)
                else: r[field] = 'wrong'
                with self.subTest(field=field, missing=missing), self.assertRaises(ValueError):
                    pilot.score(self.cases, [r], self.manifest)

    def test_manifest_source_plan_and_selection_tampering_rejected(self):
        for field in (*pilot.source_identity(), 'plan', 'both_orders', 'max_requests', 'max_seconds', 'run_id'):
            m = copy.deepcopy(self.manifest); m[field] = 'wrong'
            with self.subTest(field=field), self.assertRaises(ValueError):
                pilot.score(self.cases, [], m)
        with self.assertRaises(ValueError): pilot.score(self.cases[:1], [], self.manifest)

    def test_partial_or_inconsistent_decisions_rejected(self):
        for mutate in (lambda d:d.pop('nouls'), lambda d:d.update(selected=self.first),
                       lambda d:d['probabilities'].update(none=0.5),
                       lambda d:d['nouls'].update(extra=0.5), lambda d:d.update(confidence=True)):
            r = self.record(self.case); mutate(r['decision'])
            with self.assertRaises(ValueError): pilot.score(self.cases, [r], self.manifest)

    def test_error_records_cannot_smuggle_decisions_or_bad_status(self):
        for mutate in (lambda r:r.update(status='unknown'), lambda r:r.update(elapsed_ms=float('nan')),
                       lambda r:r.update(http_status=True), lambda r:r.update(decision={'selected':'none'})):
            r = self.record(self.case, status='http_error'); mutate(r)
            with self.assertRaises(ValueError): pilot.score(self.cases,[r],self.manifest)

    def test_reverse_order_missing_and_unplanned_records(self):
        m = pilot.run_manifest(self.cases, True, 96, 300)
        records = [self.record(self.case, manifest=m), self.record(self.case, order='reversed', manifest=m)]
        report = pilot.score(self.cases, records, m)
        self.assertEqual(len(report['missing_planned_records']),94)
        self.assertEqual(report['order_stability'], {'valid_pairs':1,'same_selection':1})
        with self.assertRaises(ValueError):
            pilot.score(self.cases, [self.record(self.case, order='reversed')], self.manifest)

    def test_empty_run_reports_missing_without_quality_pass(self):
        report = pilot.score(self.cases, [], self.manifest)
        self.assertEqual(len(report['missing_planned_records']),48)
        self.assertFalse(report['automatic_quality_approved'])
        self.assertEqual(report['metrics'],{})

    def test_run_manifest_budget_checks(self):
        for count, seconds, both in ((0,300,False),(97,300,False),(48,0,False),
                                     (48,601,False),(48,300,True),(48,300,'yes')):
            with self.subTest(count=count, seconds=seconds, both=both), self.assertRaises(ValueError):
                pilot.run_manifest(self.cases,both,count,seconds)

    def test_mixed_runs_and_legacy_records_rejected(self):
        other = pilot.run_manifest(self.cases,False,48,300)
        with self.assertRaises(ValueError):
            pilot.score(self.cases,[self.record(self.case,manifest=other)],self.manifest)
        legacy = {'case_id':self.case['id'],'order':'original',
                  'request_sha256':pilot.digest(pilot.encoded(self.req)),
                  'status':'ok','decision':{'selected':'none'}}
        with self.assertRaises(ValueError): pilot.score(self.cases,[legacy],self.manifest)

    def test_mock_http_failure_is_persisted_and_missing_run_stays_missing(self):
        cases = self.cases[:2]
        manifest = pilot.run_manifest(cases,False,2,30)
        with tempfile.TemporaryDirectory() as d:
            mf=Path(d)/'manifest.json';mf.write_text(json.dumps(manifest))
            args=types.SimpleNamespace(authorized_live=True,max_requests=2,max_seconds=30,
                                       out=str(Path(d)/'result.jsonl'),both_orders=False,run_manifest=str(mf))
            with patch.dict(os.environ,{'TYPESAFE_API_KEY':'offline-test-placeholder'}), patch('urllib.request.build_opener') as net:
                net.return_value.open.side_effect=pilot.urllib.error.HTTPError(pilot.ENDPOINT,429,'redacted',{},None)
                pilot.live(cases,args)
                records=[json.loads(line) for line in Path(args.out).read_text().splitlines()]
                report=pilot.score(cases,records,manifest)
                self.assertEqual(report['metrics']['all']['invalid_or_error'],1)
                self.assertEqual(len(report['missing_planned_records']),1)
                self.assertNotIn('offline-test-placeholder',Path(args.out).read_text())
                net.return_value.open.assert_called_once()

    def test_live_mock_records_can_be_rescored_and_budget_mismatch_sends_nothing(self):
        case = self.case
        manifest = pilot.run_manifest([case], False, 1, 30)
        with tempfile.TemporaryDirectory() as d:
            mf = Path(d)/'manifest.json'; mf.write_text(json.dumps(manifest))
            args = types.SimpleNamespace(authorized_live=True,max_requests=1,max_seconds=30,
                                         out=str(Path(d)/'result.jsonl'),both_orders=False,run_manifest=str(mf))
            from unittest.mock import MagicMock
            response = MagicMock(); response.__enter__.return_value = response
            response.read.return_value = json.dumps(self.response()).encode()
            with patch.dict(os.environ, {'TYPESAFE_API_KEY':'offline-test-placeholder'}), patch('urllib.request.build_opener') as net:
                net.return_value.open.return_value = response
                pilot.live([case],args)
                records = [json.loads(line) for line in Path(args.out).read_text().splitlines()]
                self.assertEqual(pilot.score([case],records,manifest)['records'],1)
                net.return_value.open.assert_called_once()
                args.max_seconds = 29
                net.reset_mock()
                with self.assertRaises(ValueError): pilot.live([case],args)
                net.assert_not_called()

    def test_no_live_without_key_and_authorization(self):
        args = types.SimpleNamespace(authorized_live=False, max_requests=48, max_seconds=300, out='unused', both_orders=False)
        with patch('urllib.request.build_opener') as net:
            with self.assertRaises(ValueError): pilot.live(self.cases,args)
            args.authorized_live=True
            with patch.dict(os.environ, {}, clear=True):
                with self.assertRaises(ValueError): pilot.live(self.cases,args)
            net.assert_not_called()

    def test_private_write_no_overwrite(self):
        with tempfile.TemporaryDirectory() as d:
            p=Path(d)/'evidence.json'
            pilot.private_write(p,'{}')
            self.assertEqual(p.stat().st_mode & 0o777,0o600)
            with self.assertRaises(FileExistsError):pilot.private_write(p,'bad')
            self.assertEqual(p.read_text(),'{}')

    def test_redirect_refused(self):
        with self.assertRaises(ValueError):
            pilot.NoRedirect().redirect_request(None,None,302,'',{},'https://example.com')


if __name__ == '__main__': unittest.main()
