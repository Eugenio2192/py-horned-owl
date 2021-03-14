use pyo3::prelude::*;
use pyo3::wrap_pyfunction;
use pyo3::types::{PyString,PyList};
use pyo3::PyDowncastError;
use std::io::BufReader;
use std::fs::File;

use horned_owl::vocab::{AnnotationBuiltIn,WithIRI};
use horned_owl::model::*;
use horned_owl::ontology::iri_mapped::IRIMappedOntology;
use horned_owl::ontology::axiom_mapped::AxiomMappedOntology;

use curie::PrefixMapping;

use std::collections::HashSet;
use std::collections::HashMap;
use std::time::Instant;
use std::default::Default;

#[pyclass]
#[derive(Clone,Default,Debug,PartialEq)]
struct SimpleAxiomContent {
    #[pyo3(get,set)]
    str_val: Option<String>,
    #[pyo3(get,set)]
    ax_val: Option<PySimpleAxiom>,
}

impl SimpleAxiomContent {
    fn parse(ax: Vec<PyObject>, py: Python ) -> Vec<SimpleAxiomContent> {
        let els: Vec<SimpleAxiomContent> = ax.into_iter().map(|aax: PyObject| {
            //Is aax a simple string or itelf another list? One of these should work.
            let strax: Result<&PyString,PyDowncastError> = aax.as_ref(py).downcast::<PyString>();
            let lstax: Result<&PyList,PyDowncastError> = aax.as_ref(py).downcast::<PyList>();

            if let Ok(str_val) = strax {
                SimpleAxiomContent{str_val:Some(str_val.to_string()),ax_val:None}
            } else if let Ok(list_val) = lstax {
                let pyeles: Vec<PyObject> = list_val.extract().unwrap();
                let eles: Vec<SimpleAxiomContent> = SimpleAxiomContent::parse(pyeles,py);
                SimpleAxiomContent{str_val:None,ax_val:Some(PySimpleAxiom{elements:eles})}
            } else {
                println!("Got an unparseable value: {:?}",aax);
                panic!("Unparseable axiom sent from Python to Rust.");
            }
        }
        ).collect();

        els
    }
}

impl From<String> for SimpleAxiomContent {
    fn from(s: String) -> Self {
        SimpleAxiomContent { str_val: Some(s), ax_val: None }
    }
}

impl From<&str> for SimpleAxiomContent {
    fn from(s: &str) -> Self {
        SimpleAxiomContent { str_val: Some(s.to_string()), ax_val: None }
    }
}

impl From<SimpleAxiomContent> for String {
    fn from(s: SimpleAxiomContent) -> String {
        s.str_val.unwrap()
    }
}

impl From<&SimpleAxiomContent> for String {
    fn from(s: &SimpleAxiomContent) -> String {
        s.str_val.as_ref().unwrap().clone()
    }
}

impl From<PySimpleAxiom> for SimpleAxiomContent {
    fn from(item: PySimpleAxiom) -> Self {
        SimpleAxiomContent { str_val: None, ax_val: Some(item) }
    }
}


#[pyclass]
#[derive(Clone,Default,Debug,PartialEq)]
struct PySimpleAxiom {
    elements: Vec<SimpleAxiomContent>,
}

//impl FromPyObject for PySimpleAxiom {
//    fn extract(ob: PyObject) -> PyResult<PySimpleAxiom> {
//        let elements: Vec<SimpleAxiomContent> = Vec::extract(ob);
//        PySimpleAxiom(elements)
//    }
//}

impl ToPyObject for PySimpleAxiom {

    fn to_object(&self, py: Python<'_>) -> PyObject {

        //if just one object, don't send a whole list
        if self.elements.len() ==1 {
            let ele = self.elements.iter().next().unwrap();
            if ele.str_val.is_some() {
                ele.str_val.as_ref().unwrap().to_object(py)
            } else if ele.ax_val.is_some() {
                ele.ax_val.as_ref().unwrap().to_object(py)
            } else {
                ().to_object(py)
            }
        } else { // More than one
            let list = PyList::empty(py);

            for ele in self.elements.iter() {
                if ele.str_val.is_some() {
                    list.append(ele.str_val.as_ref().unwrap().to_object(py));
                } else {
                    if ele.ax_val.is_some() {
                        list.append(ele.ax_val.as_ref().unwrap().to_object(py));
                    }
                }
            };

            list.into()
        }
    }
}

impl From<&ObjectPropertyExpression> for PySimpleAxiom {
    fn from(ope: &ObjectPropertyExpression) -> PySimpleAxiom {
        let mut pyax = PySimpleAxiom::default();

        match ope {
            ObjectPropertyExpression::ObjectProperty(p) => {
                pyax.elements.push(p.0.to_string().into());
            },
            ObjectPropertyExpression::InverseObjectProperty(p) => {
                pyax.elements.push("InverseObjectProperty".into());
                pyax.elements.push(p.0.to_string().into());
            }
        }

        pyax
    }
}

impl From<&ClassExpression> for PySimpleAxiom {
    fn from(ce: &ClassExpression) -> PySimpleAxiom {
        let mut pyax = PySimpleAxiom::default();

            match ce {
                ClassExpression::Class(c) => {
                    pyax.elements.push(c.0.to_string().into());
                },
                ClassExpression::ObjectIntersectionOf(clsses) => {
                    pyax.elements.push("ObjectIntersectionOf".into());
                    for ele in clsses {
                        pyax.elements.push(PySimpleAxiom::from(ele).into());
                    }
                },
                ClassExpression::ObjectComplementOf(ce) => {
                    pyax.elements.push("ObjectComplementOf".into());
                    pyax.elements.push(PySimpleAxiom::from(&(**ce)).into());
                },
                ClassExpression::ObjectSomeValuesFrom{ope,bce} => {
                    pyax.elements.push("ObjectSomeValuesFrom".into());
                    pyax.elements.push(PySimpleAxiom::from(ope).into());
                    pyax.elements.push(PySimpleAxiom::from(&(**bce)).into());
                },
                ClassExpression::ObjectAllValuesFrom{ope,bce} => {
                    pyax.elements.push("ObjectAllValuesFrom".into());
                    pyax.elements.push(PySimpleAxiom::from(ope).into());
                    pyax.elements.push(PySimpleAxiom::from(&(**bce)).into());
                },
                _ => ()
            }
        pyax
    }
}

impl From<&SimpleAxiomContent> for ClassExpression {
    fn from(ce: &SimpleAxiomContent) -> ClassExpression {
        let b = Build::new();

        if let Some(axval) = &ce.ax_val {
            //Parse axiom into class expression
            //It has some elements, the first of which should be the type of the expression
            let mut eles = axval.elements.iter();
            let cename: String = eles.next().unwrap().into();

            match &cename[..] {
                "ObjectSomeValuesFrom" => {
                    //First an object property
                    let objpname = eles.next().unwrap();
                    let obp = b.object_property(b.iri(objpname.clone()));
                    let obpe = ObjectPropertyExpression::ObjectProperty(obp.clone());

                    //Then its target
                    let objptar = eles.next().unwrap();

                    ClassExpression::ObjectSomeValuesFrom{
                                    ope: obpe,
                               bce: b.class(objptar).into()
                    }
                },
                _ => {
                    println!("Unknown class expression name: {:?}",cename);
                    panic!("Unknown class expression")
                }
            }
        } else if let Some(strval) = &ce.str_val {
            //Parse string value into a simple class
            ClassExpression::Class(Class(b.iri(strval.clone())))
        } else {

            panic!("Unparseable class expression")
        }
    }
}

impl From<PySimpleAxiom> for Axiom {
    fn from(ax: PySimpleAxiom) -> Axiom {
        let b = Build::new();
        //we expect a List with elements
        let mut eles = ax.elements.iter();
        //The first of which is the axiom type
        let axtype: String = eles.next().unwrap().into();

        let resax : Axiom = match &axtype[..] {
            "DeclareClass" => {
                println!("DeclareClass");
                //next is going to be an IRI of the class being declared
                let clsiri: String = eles.next().unwrap().into();
                Axiom::DeclareClass(DeclareClass(Class(b.iri(clsiri.clone()))))
            },
            "SubClassOf" => {
                println!("SubClassOf");
                //next is going to be an IRI for the class that is the subclass
                let subiri: String = eles.next().unwrap().into();
                let subce: ClassExpression = ClassExpression::Class(Class(b.iri(subiri.clone())));
                //then either class expression for the superclass, or another list to iterate over.
                let ce: &SimpleAxiomContent = eles.next().unwrap();

                //Parse a class expression from the simple axiom content
                let supce: ClassExpression = ce.into();

                //Create the Axiom
                Axiom::SubClassOf(SubClassOf{sup:supce,sub:subce})

            },
            "AnnotationAssertion" => {
                println!("AnnotationAssertion");
                let subiri: String = eles.next().unwrap().into();
                let apiri: String = eles.next().unwrap().into();
                let annstr: String = eles.next().unwrap().into();

                Axiom::AnnotationAssertion(AnnotationAssertion{subject:b.iri(subiri.clone()),
                        ann: Annotation{ap: AnnotationProperty(b.iri(apiri))
                            ,av: AnnotationValue::Literal(Literal::Simple{literal:annstr})}})
            },
            _ => {
                println!("Something else: {:?}",axtype);
                Axiom::DeclareClass(DeclareClass(Class(b.iri("Eh?"))))
            },
        };
        //

        resax
    }
}

impl From<&Axiom> for PySimpleAxiom {

    fn from(aax: &Axiom) -> PySimpleAxiom {
        let mut pyax = PySimpleAxiom::default();
        pyax.elements.push(format!("{}",aax.kind()).into());

        match aax {
            Axiom::DeclareClass(DeclareClass(dc)) => {
                pyax.elements.push( dc.0.to_string().into() );
            },
            Axiom::SubClassOf(SubClassOf{sup,sub}) => {
                pyax.elements.push( PySimpleAxiom::from(sub).into() );
                pyax.elements.push( PySimpleAxiom::from(sup).into() );
            },
            Axiom::AnnotationAssertion(AnnotationAssertion{subject,ann:Annotation{ap,av}}) => {
                pyax.elements.push( subject.to_string().into() );
                pyax.elements.push( ap.0.to_string().into() );
                let av: String = match av {
                    AnnotationValue::Literal(lit) => lit.literal().to_string(),
                    AnnotationValue::IRI(iri) => iri.to_string(),
                };
                pyax.elements.push( av.into() );
            },
            Axiom::EquivalentClasses(EquivalentClasses(clsses)) => {
                for ele in clsses {
                    pyax.elements.push( PySimpleAxiom::from(ele).into() );
                }
            },
            _ => ()
        }

        pyax
    }
}

#[pyclass]
#[derive(Default)]
struct PyIndexedOntology {

    //State variables private to Rust, exposed through methods to Python
    labels_to_iris: HashMap<String,IRI>,

    classes_to_subclasses: HashMap<IRI,HashSet<IRI>>, //axiom typed index would give subclass axioms
    classes_to_superclasses: HashMap<IRI,HashSet<IRI>>,
    //classes: HashSet<IRI>, //declaration typed index in horned-owl

    //The primary store of the axioms is a Horned OWL indexed ontology
    ontology: IRIMappedOntology,
    //Need this for saving again afterwards
    mapping: PrefixMapping,
}

#[pymethods]
impl PyIndexedOntology {
    fn set_label(&mut self, iri: String, label: String) -> PyResult<()> {
        let b = Build::new();
        let iri = b.iri(iri);

        let ax1:AnnotatedAxiom =
            Axiom::AnnotationAssertion(
                AnnotationAssertion{subject:iri.clone(),
                    ann: Annotation{ap: b.annotation_property(AnnotationBuiltIn::LABEL.iri_s()),
                    av: AnnotationValue::Literal(
                        Literal::Simple{literal:label.clone()})}}).into();

        //If we already have a label, update it:
        let old_ax = &self.ontology.get_axs_for_iri(iri).filter_map(|aax: &AnnotatedAxiom| {
            match &aax.axiom {
                Axiom::AnnotationAssertion(AnnotationAssertion{subject:_subj,ann}) => {
                        match ann {
                            Annotation {ap, av:  AnnotationValue::Literal(Literal::Simple{literal:_old}) } => {
                                if AnnotationBuiltIn::LABEL.iri_s().eq(&ap.0.to_string()) {
                                    Some(aax.clone())
                                } else {
                                    None
                                }
                            },
                            _ => None,
                        }
                    },
                    _ => None,
                }
        }).next();

        if let Some(old_ax) = old_ax {
            self.ontology.update_axiom(old_ax, ax1);
        } else {
        //If no label already, just add one
            self.ontology.insert(ax1);
        }
        Ok(())
    }

    fn get_iri_for_label(&mut self, label: String) -> PyResult<PyObject> {
        let gil = Python::acquire_gil();
        let py = gil.python();
        let iri_value = &self.labels_to_iris.get(&label);
        if let Some(iri_value) = iri_value {
            Ok(iri_value.to_string().to_object(py))
        } else {
            Ok(().to_object(py))
        }
    }

    fn get_subclasses(&mut self, iri: String) -> PyResult<HashSet<String>> {
        let b = Build::new();
        let iri = b.iri(iri);

        let subclasses = self.classes_to_subclasses.get(&iri);
        if let Some(subclss) = subclasses {
            let subclasses : HashSet<String> = subclss.iter().map(|sc| sc.to_string()).collect();
            Ok(subclasses)
        } else {
            Ok(HashSet::new())
        }
    }

    fn get_superclasses(&mut self, iri: String) -> PyResult<HashSet<String>> {
        let b = Build::new();
        let iri = b.iri(iri);

        let superclasses  = self.classes_to_superclasses.get(&iri);
        if let Some(superclss) = superclasses {
            let superclasses : HashSet<String> = superclss
                   .iter().map(|sc| sc.to_string()).collect();
            Ok(superclasses)
        } else {
            Ok(HashSet::new())
        }
    }

    fn get_classes(&mut self) -> PyResult<HashSet<String>> {
        //Get the DeclareClass axioms
        let classes = self.ontology.k().annotated_axiom(AxiomKind::DeclareClass);

        let classes : HashSet<String> = classes
                        .filter_map(|aax| {
                            match aax.clone().axiom {
                                    Axiom::DeclareClass(dc) => {
                                        Some(dc.0.0.to_string())
                                    },
                                    _ => None
                                }
                        }).collect();
        Ok(classes)
    }

    fn get_annotation(&mut self, class_iri: String, ann_iri: String) -> PyResult<PyObject> {
        let b = Build::new();
        let iri = b.iri(class_iri);

        let gil = Python::acquire_gil();
        let py = gil.python();

        let literal_values = &self.ontology.get_axs_for_iri(iri)
                                .filter_map(|aax: &AnnotatedAxiom| {
            match &aax.axiom {
                Axiom::AnnotationAssertion(AnnotationAssertion{subject:_,ann}) => {
                        match ann {
                            Annotation {ap, av:  AnnotationValue::Literal(Literal::Simple{literal}) } => {
                                if ann_iri.eq(&ap.0.to_string()) {
                                    Some(literal.clone())
                                } else {
                                    None
                                }
                            },
                            _ => None,
                        }
                    },
                    _ => None,
                }
        }).next();

        if let Some(literal_value) = literal_values {
            Ok(literal_value.to_object(py))
        } else {
            Ok(().to_object(py))
        }
    }

    fn save_to_file(&mut self, file_name: String) -> PyResult<()>{
        let before = Instant::now();

        let mut file = File::create(file_name)?;
        let mut amo : AxiomMappedOntology = AxiomMappedOntology::default();
        //Copy the axioms into an AxiomMappedOntology as that is what horned owl writes
        for aax in self.ontology.k() {
            amo.insert(aax.clone());
        }
        let time_middle = before.elapsed().as_secs();
        println!("Finished preparing ontology for saving in {:?} seconds.", time_middle);
        let before = Instant::now();

        let result = horned_owl::io::owx::writer::write(&mut file, &amo, Some(&self.mapping));

        let time_after = before.elapsed().as_secs();
        println!("Finished saving ontology to file in  {:?} seconds.", time_after);

        match result {
            Ok(()) => Ok(()),
            Err(error) => panic!("Problem saving the ontology to a file: {:?}", error),
        }
    }

    fn get_axioms_for_iri(&mut self, iri: String) -> PyResult<Vec<PyObject>> {
        let b = Build::new();
        let iri = b.iri(iri);

        let gil = Python::acquire_gil();
        let py = gil.python();

        let axioms = self.ontology.get_axs_for_iri(iri)
                                .filter_map(|aax: &AnnotatedAxiom| {
                                    Some(PySimpleAxiom::from(&aax.axiom))
                                }).map(|aax: PySimpleAxiom| {aax.to_object(py)}).collect();

        Ok(axioms)
    }

    fn get_axioms(&mut self) -> PyResult<Vec<PyObject>> {
        let gil = Python::acquire_gil();
        let py = gil.python();

        let axioms = self.ontology.i().iter()
                                .filter_map(|aax: &AnnotatedAxiom| {
                                    Some(PySimpleAxiom::from(&aax.axiom))
                                }).map(|aax: PySimpleAxiom| {aax.to_object(py)}).collect();

        Ok(axioms)
    }

    fn add_axiom(&mut self, ax: Vec<PyObject>) -> PyResult<()> {
        let gil = Python::acquire_gil();
        let py = gil.python();

        let els: Vec<SimpleAxiomContent> = SimpleAxiomContent::parse(ax,py);

        let ax: Axiom = PySimpleAxiom{elements:els}.into();

        self.ontology.insert(ax);

        Ok(())
    }

    fn remove_axiom(&mut self, ax: Vec<PyObject>) -> PyResult<()> {
        let gil = Python::acquire_gil();
        let py = gil.python();

        let els: Vec<SimpleAxiomContent> = SimpleAxiomContent::parse(ax,py);

        let ax: Axiom = PySimpleAxiom{elements:els}.into();

        self.ontology.remove(&ax.into());

        Ok(())
    }
}

impl PyIndexedOntology {
    fn insert(&mut self, ax: &AnnotatedAxiom) -> () {
        match ax.kind() {
            AxiomKind::AnnotationAssertion => {
                match ax.clone().axiom {
                    Axiom::AnnotationAssertion(AnnotationAssertion{subject,ann}) => {
                        match ann {
                            Annotation {ap, av:  AnnotationValue::Literal(Literal::Simple{literal}) } => {
                                if AnnotationBuiltIn::LABEL.iri_s().eq(&ap.0.to_string()) {
                                    &self.labels_to_iris.insert(literal.clone(),subject.clone());
                                }
                            },
                            _ => (),
                        }
                    },
                    _ => (),
                }
            },
            AxiomKind::SubClassOf => {
                match ax.clone().axiom {
                    Axiom::SubClassOf(SubClassOf{sup,sub}) => {
                        match sup {
                            ClassExpression::Class(c) => {
                                match sub {
                                    ClassExpression::Class(d) => { //Direct subclasses only
                                        &self.classes_to_subclasses.entry(c.0.clone()).or_insert(HashSet::new()).insert(d.0.clone());
                                        &self.classes_to_superclasses.entry(d.0.clone()).or_insert(HashSet::new()).insert(c.0.clone());
                                    },
                                    _ => ()
                                }
                            },
                        _ => ()
                        }
                    },
                    _ => ()
                }
            },
            _ => ()
        }
    }

    fn from(iro: IRIMappedOntology) -> PyIndexedOntology {
        let mut ino = PyIndexedOntology::default();

        for ax in iro.i() {
            ino.insert(&ax);
        }

        ino.ontology = iro;

        ino
    }

}

#[pyfunction]
fn open_ontology(ontoname: &PyString) -> PyResult<PyIndexedOntology> {
    let before = Instant::now();

    let filename: String = ontoname.extract().unwrap();

    let f = File::open(filename).ok().unwrap();
    let mut f = BufReader::new(f);

    let r = horned_owl::io::owx::reader::read(&mut f);
    assert!(r.is_ok(), "Expected ontology, got failure:{:?}", r.err());

    let time_middle = before.elapsed().as_secs();
    let (o, m) = r.ok().unwrap();
    println!("Finished reading ontology from file in {:?} seconds.", time_middle);

    let before = Instant::now();
    println!("About to build indexes");
    let iro = IRIMappedOntology::from(o);

    let mut lo =  PyIndexedOntology::from(iro);
    lo.mapping = m; //Needed when saving

    let time_after = before.elapsed().as_secs();
    println!("Finished building indexes in  {:?} seconds.", time_after);

    Ok(lo)
}

#[pyfunction]
fn get_descendants(onto: &PyIndexedOntology, parent: &PyString) -> PyResult<HashSet<String>> {
    let mut descendants = HashSet::new();
    let parent: String = parent.extract().unwrap();

    let b = Build::new();
    let parentiri = b.iri(parent);

    recurse_descendants(onto, &parentiri, &mut descendants);

    Ok(descendants)
}

fn recurse_descendants(onto : &PyIndexedOntology, superclass: &IRI, descendants: &mut HashSet<String>) {
    descendants.insert(superclass.into());
    if onto.classes_to_subclasses.contains_key(superclass) {
        for cls2 in &mut onto.classes_to_subclasses[superclass].iter() {
            recurse_descendants(onto, cls2, descendants);
        }
    }
}

#[pyfunction]
fn get_ancestors(onto: &PyIndexedOntology, child: &PyString) -> PyResult<HashSet<String>> {
    let mut ancestors = HashSet::new();
    let child: String = child.extract().unwrap();

    let b = Build::new();
    let childiri = b.iri(child);

    recurse_ancestors(onto, &childiri, &mut ancestors);

    Ok(ancestors)
}

fn recurse_ancestors(onto : &PyIndexedOntology, subclass: &IRI, ancestors: &mut HashSet<String>) {
    ancestors.insert(subclass.into());
    if onto.classes_to_superclasses.contains_key(subclass) {
        for cls2 in &mut onto.classes_to_superclasses[subclass].iter() {
            recurse_ancestors(onto, cls2, ancestors);
        }
    }
}

#[pymodule]
fn ontopyo3(_py:Python, m:&PyModule) -> PyResult<()> {
    m.add_class::<PyIndexedOntology>()?;

    m.add_function(wrap_pyfunction!(open_ontology,m)?)?;
    m.add_function(wrap_pyfunction!(get_descendants,m)?)?;
    m.add_function(wrap_pyfunction!(get_ancestors,m)?)?;

    Ok(())
}